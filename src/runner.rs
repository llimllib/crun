use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use jiff::Zoned;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::cli::Args;
use crate::color::{assign_colors, colorize};
use crate::command::{shell_command, CommandInfo, CommandState};
use crate::input;
use crate::output::{format_line, format_prefix, max_prefix_inner_width, pad_prefix, PrefixStyle};

/// Events emitted by child processes.
#[derive(Debug)]
enum Event {
    /// A line of output (stdout or stderr) from a command.
    Output { index: usize, line: String },
    /// A command has started, with its PID.
    Started { index: usize, pid: Option<u32> },
    /// A command has exited.
    Exited { index: usize, state: CommandState },
}

/// Restart delay configuration.
#[derive(Debug, Clone, PartialEq)]
enum RestartDelay {
    /// Fixed delay in milliseconds.
    Fixed(u64),
    /// Exponential backoff (100ms, 200ms, 400ms, ...).
    Exponential,
}

/// Parse the restart-after argument value.
///
/// Returns the restart delay configuration:
/// - "exponential" -> Exponential backoff
/// - A number -> Fixed delay in milliseconds
/// - Invalid/empty -> No delay (0ms)
fn parse_restart_delay(value: &str) -> RestartDelay {
    if value.eq_ignore_ascii_case("exponential") {
        RestartDelay::Exponential
    } else if let Ok(ms) = value.parse::<u64>() {
        RestartDelay::Fixed(ms)
    } else {
        RestartDelay::Fixed(0)
    }
}

/// Calculate the actual delay in milliseconds for a restart.
///
/// For fixed delays, returns the configured value.
/// For exponential backoff, returns 100ms * 2^restart_count (capped at ~1 minute).
fn calculate_restart_delay(delay: &RestartDelay, restart_count: i32) -> u64 {
    match delay {
        RestartDelay::Fixed(ms) => *ms,
        RestartDelay::Exponential => {
            // Base delay of 100ms, doubling each time, capped at ~1 minute
            let exp = restart_count.min(9) as u32; // 2^9 * 100 = 51200ms
            100 * 2u64.pow(exp)
        }
    }
}

/// Parse the max-processes argument value.
///
/// Returns the maximum number of concurrent processes to run.
/// - If `None`, returns the number of commands (no limit).
/// - If a number like "4", returns that number.
/// - If a percentage like "50%", returns that percentage of available CPUs (minimum 1).
fn parse_max_processes(value: Option<&str>, num_commands: usize) -> usize {
    match value {
        None => num_commands,
        Some(s) => {
            if let Some(percent_str) = s.strip_suffix('%') {
                // Percentage of available CPUs
                if let Ok(percent) = percent_str.parse::<f64>() {
                    let num_cpus = std::thread::available_parallelism()
                        .map(|p| p.get())
                        .unwrap_or(1);
                    let result = (num_cpus as f64 * percent / 100.0).round() as usize;
                    result.max(1) // At least 1
                } else {
                    num_commands // Invalid percentage, no limit
                }
            } else if let Ok(n) = s.parse::<usize>() {
                if n == 0 {
                    num_commands // 0 means no limit
                } else {
                    n
                }
            } else {
                num_commands // Invalid value, no limit
            }
        }
    }
}

/// Run the commands according to the provided arguments.
/// Returns the exit code that should be used.
pub async fn run(args: Args) -> anyhow::Result<i32> {
    let names = args.get_names();
    let hide_list = args.get_hide();
    let raw = args.raw;
    let kill_others = args.kill_others;
    let kill_others_on_fail = args.kill_others_on_fail;
    let kill_signal = args.kill_signal.clone();
    let kill_timeout = args.kill_timeout;
    let group = args.group;
    let timings = args.timings;
    let restart_tries = args.restart_tries;
    let restart_after = parse_restart_delay(&args.restart_after);
    let handle_input = args.handle_input;
    let prefix_length = args.prefix_length;
    let do_pad = args.pad_prefix;
    let timestamp_format = args.timestamp_format.clone();

    // Get the final list of commands (with placeholder expansion if -P is used)
    let command_lines: Vec<String> = args.get_commands();

    // Build command infos
    let mut commands: Vec<CommandInfo> = command_lines
        .iter()
        .enumerate()
        .map(|(i, cmd_line)| {
            let name = names.get(i).cloned().unwrap_or_else(|| i.to_string());
            CommandInfo::new(i, name, cmd_line.clone())
        })
        .collect();

    let num_commands = commands.len();

    // Assign colors to commands
    let colors = assign_colors(&args.prefix_colors, num_commands, args.no_color || raw);

    // Determine prefix style
    let prefix_style = if raw {
        PrefixStyle::None
    } else {
        PrefixStyle::from_arg(args.prefix.as_deref(), !names.is_empty())
    };

    // Pre-compute prefixes for padding
    let is_template = prefix_style.is_template();
    let prefixes: Vec<String> = commands
        .iter()
        .map(|cmd| format_prefix(cmd, &prefix_style, None, prefix_length, &timestamp_format))
        .collect();
    let pad_width = if do_pad {
        max_prefix_inner_width(&prefixes, is_template)
    } else {
        0
    };

    // Channel for events from child tasks
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();

    // Parse max-processes to limit concurrency
    let max_procs = parse_max_processes(args.max_processes.as_deref(), num_commands);

    // Track PIDs per command, shared with the signal handler
    let pids: Arc<std::sync::Mutex<HashMap<usize, u32>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));

    // Track whether we received SIGINT (to treat exit codes as 0)
    let caught_sigint = Arc::new(AtomicBool::new(false));

    // Set up signal forwarding: when crun receives SIGINT/SIGTERM, forward to all children
    #[cfg(unix)]
    {
        let pids_for_signal = Arc::clone(&pids);
        let caught_sigint_for_handler = Arc::clone(&caught_sigint);
        tokio::spawn(async move {
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .expect("failed to register SIGINT handler");
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");
            let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("failed to register SIGHUP handler");

            let signal_num = tokio::select! {
                _ = sigint.recv() => {
                    caught_sigint_for_handler.store(true, Ordering::SeqCst);
                    libc::SIGINT
                }
                _ = sigterm.recv() => libc::SIGTERM,
                _ = sighup.recv() => libc::SIGHUP,
            };

            // Forward the signal to all child process groups
            let pids = pids_for_signal.lock().unwrap();
            for pid in pids.values() {
                unsafe {
                    libc::kill(-(*pid as i32), signal_num);
                }
            }
        });
    }

    // Set up Ctrl+C handling on Windows
    #[cfg(windows)]
    {
        let pids_for_signal = Arc::clone(&pids);
        let caught_sigint_for_handler = Arc::clone(&caught_sigint);
        tokio::spawn(async move {
            // Wait for Ctrl+C
            if tokio::signal::ctrl_c().await.is_ok() {
                caught_sigint_for_handler.store(true, Ordering::SeqCst);

                // Kill all child process trees using taskkill
                let pids = pids_for_signal.lock().unwrap();
                for pid in pids.values() {
                    kill_process_tree(*pid);
                }
            }
        });
    }

    // Track order of completion for --success first/last
    let mut exit_order: Vec<(usize, i32)> = Vec::new();

    // Track which commands have exited (final, not restarting)
    let mut exited: Vec<bool> = vec![false; num_commands];

    // Track whether we've already initiated the kill-others sequence
    let mut kill_initiated = false;

    // Track restart counts
    let mut restart_counts: Vec<i32> = vec![0; num_commands];

    // Track timing info
    let mut started_at: Vec<Option<(Instant, Zoned)>> = vec![None; num_commands];
    let mut ended_at: Vec<Option<(Instant, Zoned)>> = vec![None; num_commands];

    // For --group: buffer output per command, active index tracks sequential flushing
    let mut group_buffers: Vec<Vec<String>> = vec![Vec::new(); num_commands];
    let mut group_active_index: usize = 0;
    let mut group_exited: Vec<bool> = vec![false; num_commands];

    // Parse default input target
    let default_input_target: usize = args.default_input_target.parse().unwrap_or(0);

    // Set up stdin routing if --handle-input
    let mut stdin_receivers: HashMap<usize, mpsc::UnboundedReceiver<String>> = HashMap::new();

    if handle_input {
        let mut stdin_senders: HashMap<usize, mpsc::UnboundedSender<String>> = HashMap::new();
        for i in 0..num_commands {
            let (sender, receiver) = mpsc::unbounded_channel();
            stdin_senders.insert(i, sender);
            stdin_receivers.insert(i, receiver);
        }
        input::spawn_input_router(default_input_target, stdin_senders);
    }

    // Queue of commands waiting to be spawned (for max-processes limiting)
    // We'll spawn up to max_procs initially, then spawn more as commands finish
    let mut pending_commands: std::collections::VecDeque<usize> = (0..num_commands).collect();

    // Spawn initial batch of commands (up to max_procs)
    let initial_batch_size = max_procs.min(num_commands);
    for _ in 0..initial_batch_size {
        if let Some(i) = pending_commands.pop_front() {
            let tx = tx.clone();
            let cmd_line = command_lines[i].clone();
            let stdin_rx = stdin_receivers.remove(&i);
            tokio::spawn(async move {
                spawn_command(i, &cmd_line, tx, stdin_rx, handle_input).await;
            });
        }
    }

    // Process events
    let mut final_count = 0;
    while final_count < num_commands {
        let event = match rx.recv().await {
            Some(e) => e,
            None => break,
        };
        match event {
            Event::Started { index, pid } => {
                if let Some(p) = pid {
                    pids.lock().unwrap().insert(index, p);
                }
                commands[index].state = CommandState::Running;
                started_at[index] = Some((Instant::now(), Zoned::now()));

                // Log timing start
                if timings && !raw {
                    let prefix = make_prefix(
                        &commands[index],
                        &prefix_style,
                        pids.lock().unwrap().get(&index).copied(),
                        prefix_length,
                        do_pad,
                        pad_width,
                        &colors[index],
                        &timestamp_format,
                    );
                    let ts = started_at[index]
                        .as_ref()
                        .unwrap()
                        .1
                        .strftime("%Y-%m-%d %H:%M:%S%.3f");
                    let msg = format!("{} started at {}", commands[index].command_line, ts);
                    output_line(
                        &msg,
                        &prefix,
                        group,
                        index,
                        group_active_index,
                        &mut group_buffers,
                    );
                }
            }
            Event::Output { index, line } => {
                if should_hide(index, &commands[index].name, &hide_list) {
                    continue;
                }

                let prefix = if raw {
                    String::new()
                } else {
                    make_prefix(
                        &commands[index],
                        &prefix_style,
                        pids.lock().unwrap().get(&index).copied(),
                        prefix_length,
                        do_pad,
                        pad_width,
                        &colors[index],
                        &timestamp_format,
                    )
                };

                output_line(
                    &line,
                    &prefix,
                    group,
                    index,
                    group_active_index,
                    &mut group_buffers,
                );
            }
            Event::Exited { index, state } => {
                let code = match &state {
                    CommandState::Exited { code } => *code,
                    CommandState::Killed { .. } => 1,
                    CommandState::Errored { .. } => 1,
                    _ => 0,
                };

                commands[index].state = state;
                ended_at[index] = Some((Instant::now(), Zoned::now()));

                // Log timing stop
                if timings && !raw {
                    let prefix = make_prefix(
                        &commands[index],
                        &prefix_style,
                        pids.lock().unwrap().get(&index).copied(),
                        prefix_length,
                        do_pad,
                        pad_width,
                        &colors[index],
                        &timestamp_format,
                    );
                    let ts = ended_at[index]
                        .as_ref()
                        .unwrap()
                        .1
                        .strftime("%Y-%m-%d %H:%M:%S%.3f");
                    let duration_ms = ended_at[index]
                        .as_ref()
                        .unwrap()
                        .0
                        .duration_since(started_at[index].as_ref().unwrap().0)
                        .as_millis();
                    let msg = format!(
                        "{} stopped at {} after {}ms",
                        commands[index].command_line, ts, duration_ms
                    );
                    output_line(
                        &msg,
                        &prefix,
                        group,
                        index,
                        group_active_index,
                        &mut group_buffers,
                    );
                }

                // Check if we should restart
                if code != 0 && should_restart(restart_tries, restart_counts[index]) {
                    let prefix = make_prefix(
                        &commands[index],
                        &prefix_style,
                        pids.lock().unwrap().get(&index).copied(),
                        prefix_length,
                        do_pad,
                        pad_width,
                        &colors[index],
                        &timestamp_format,
                    );

                    // Log exit
                    if !raw && !should_hide(index, &commands[index].name, &hide_list) {
                        let exit_msg = format_exit_message(&commands[index]);
                        if !exit_msg.is_empty() {
                            output_line(
                                &exit_msg,
                                &prefix,
                                group,
                                index,
                                group_active_index,
                                &mut group_buffers,
                            );
                        }
                    }

                    // Log restart
                    if !raw && !should_hide(index, &commands[index].name, &hide_list) {
                        let restart_msg = format!("{} restarted", commands[index].command_line);
                        output_line(
                            &restart_msg,
                            &prefix,
                            group,
                            index,
                            group_active_index,
                            &mut group_buffers,
                        );
                    }

                    let current_restart = restart_counts[index];
                    restart_counts[index] += 1;
                    commands[index].state = CommandState::Pending;

                    // Respawn the command (with delay if configured)
                    let tx_clone = tx.clone();
                    let cmd_line = command_lines[index].clone();
                    let delay_ms = calculate_restart_delay(&restart_after, current_restart);
                    tokio::spawn(async move {
                        if delay_ms > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        spawn_command(index, &cmd_line, tx_clone, None, false).await;
                    });

                    continue; // Don't count as final exit
                }

                exited[index] = true;
                exit_order.push((index, code));
                final_count += 1;

                // Spawn next pending command if any (for max-processes limiting)
                if let Some(next_index) = pending_commands.pop_front() {
                    let tx_clone = tx.clone();
                    let cmd_line = command_lines[next_index].clone();
                    let stdin_rx = stdin_receivers.remove(&next_index);
                    tokio::spawn(async move {
                        spawn_command(next_index, &cmd_line, tx_clone, stdin_rx, handle_input)
                            .await;
                    });
                }

                // Log exit (unless raw mode or hidden)
                if !raw && !should_hide(index, &commands[index].name, &hide_list) {
                    let prefix = make_prefix(
                        &commands[index],
                        &prefix_style,
                        pids.lock().unwrap().get(&index).copied(),
                        prefix_length,
                        do_pad,
                        pad_width,
                        &colors[index],
                        &timestamp_format,
                    );
                    let exit_msg = format_exit_message(&commands[index]);
                    if !exit_msg.is_empty() {
                        output_line(
                            &exit_msg,
                            &prefix,
                            group,
                            index,
                            group_active_index,
                            &mut group_buffers,
                        );
                    }
                }

                // Flush group buffers
                if group {
                    group_exited[index] = true;
                    if index == group_active_index {
                        flush_group_buffers(
                            &mut group_active_index,
                            &mut group_buffers,
                            &group_exited,
                            num_commands,
                        );
                    }
                }

                // Handle --kill-others / --kill-others-on-fail
                let should_kill = kill_others || (kill_others_on_fail && code != 0);
                if should_kill && !kill_initiated {
                    kill_initiated = true;
                    let remaining = count_killable_processes(index, &exited, &pids);
                    if remaining > 0 {
                        println!(
                            "{}",
                            format_line(
                                "-->",
                                &format!("Sending {} to other processes..", kill_signal)
                            )
                        );
                        kill_other_processes(index, &exited, &pids, &kill_signal);

                        // Schedule SIGKILL escalation if timeout is set and signal is not already SIGKILL
                        #[cfg(unix)]
                        if let Some(timeout_ms) = kill_timeout {
                            if parse_signal(&kill_signal) != libc::SIGKILL {
                                let pids_for_kill = Arc::clone(&pids);
                                tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        timeout_ms,
                                    ))
                                    .await;
                                    force_kill_remaining(&pids_for_kill);
                                });
                            }
                        }
                        #[cfg(windows)]
                        {
                            // On Windows, taskkill /F already force-kills, so no escalation needed
                            let _ = kill_timeout;
                        }
                    }
                }
            }
        }
    }

    // Print timings table if requested
    if timings {
        print_timings_table(&commands, &started_at, &ended_at);
    }

    // Run teardown commands
    if !args.teardown.is_empty() {
        run_teardown_commands(&args.teardown).await;
    }

    // If we caught SIGINT, exit 0 (matching concurrently's behavior)
    if caught_sigint.load(Ordering::SeqCst) {
        return Ok(0);
    }

    // Determine exit code based on --success flag
    let exit_code = determine_exit_code(&args.success, &commands, &exit_order);
    Ok(exit_code)
}

/// Output a line, either directly or to a group buffer.
/// In group mode, only the active command's output is written directly;
/// other commands buffer their output.
fn output_line(
    msg: &str,
    prefix: &str,
    group: bool,
    index: usize,
    active_index: usize,
    buffers: &mut [Vec<String>],
) {
    let formatted = format_line(prefix, msg);
    if group {
        if index <= active_index {
            println!("{}", formatted);
        } else {
            buffers[index].push(formatted);
        }
    } else {
        println!("{}", formatted);
    }
}

/// Flush group buffers starting from the next command after the one that just exited.
/// Advances `active_index` past any already-exited commands.
fn flush_group_buffers(
    active_index: &mut usize,
    buffers: &mut [Vec<String>],
    group_exited: &[bool],
    num_commands: usize,
) {
    for i in (*active_index)..num_commands {
        *active_index = i;
        // Flush this buffer
        for line in buffers[i].drain(..) {
            println!("{}", line);
        }
        // If this command hasn't exited yet, stop here
        if !group_exited[i] {
            break;
        }
    }
}

/// Check if a command should be restarted.
fn should_restart(restart_tries: i32, current_restarts: i32) -> bool {
    if restart_tries < 0 {
        // Negative means infinite restarts
        true
    } else {
        current_restarts < restart_tries
    }
}

/// Count how many processes are still killable (not exited, not the one that triggered the kill).
fn count_killable_processes(
    except_index: usize,
    exited: &[bool],
    pids: &Arc<std::sync::Mutex<HashMap<usize, u32>>>,
) -> usize {
    let pids = pids.lock().unwrap();
    pids.iter()
        .filter(|(index, _)| **index != except_index && !exited[**index])
        .count()
}

/// Kill all running processes except the one at `except_index`.
///
/// On Unix, sends the specified signal to the process group (negative PID) so that child processes
/// spawned by the shell are also terminated.
/// On Windows, uses `taskkill /T /F` to kill the process tree (signal is ignored).
fn kill_other_processes(
    except_index: usize,
    exited: &[bool],
    pids: &Arc<std::sync::Mutex<HashMap<usize, u32>>>,
    #[allow(unused_variables)] kill_signal: &str,
) {
    let pids = pids.lock().unwrap();
    for (index, pid) in pids.iter() {
        if *index != except_index && !exited[*index] {
            #[cfg(unix)]
            {
                let signal = parse_signal(kill_signal);
                // Kill the process group (negative PID) so shell children also receive the signal
                unsafe {
                    libc::kill(-(*pid as i32), signal);
                }
            }
            #[cfg(windows)]
            {
                kill_process_tree(*pid);
            }
        }
    }
}

/// Send SIGKILL to all processes that are still running.
///
/// This is called after the kill timeout expires, to force-terminate any processes
/// that didn't respond to the initial signal.
#[cfg(unix)]
fn force_kill_remaining(pids: &Arc<std::sync::Mutex<HashMap<usize, u32>>>) {
    let pids_guard = pids.lock().unwrap();
    // Collect PIDs that are still alive (process group exists)
    let mut killable_pids = Vec::new();
    for pid in pids_guard.values() {
        // Check if the process group still exists by sending signal 0
        let pgid = -(*pid as i32);
        if unsafe { libc::kill(pgid, 0) } == 0 {
            killable_pids.push(*pid);
        }
    }
    drop(pids_guard);

    if !killable_pids.is_empty() {
        println!(
            "{}",
            crate::output::format_line(
                "-->",
                &format!("Sending SIGKILL to {} processes..", killable_pids.len())
            )
        );
        for pid in killable_pids {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

/// Kill a process and all its descendants on Windows using taskkill.
///
/// Uses `taskkill /pid <pid> /T /F` where:
/// - `/T` kills the process tree (all child processes)
/// - `/F` forcefully terminates the processes
#[cfg(windows)]
fn kill_process_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/pid", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Build the prefix string for a command.
#[allow(clippy::too_many_arguments)]
fn make_prefix(
    cmd: &CommandInfo,
    style: &PrefixStyle,
    pid: Option<u32>,
    prefix_length: usize,
    do_pad: bool,
    pad_width: usize,
    color: &str,
    timestamp_format: &str,
) -> String {
    let p = format_prefix(cmd, style, pid, prefix_length, timestamp_format);
    let is_template = style.is_template();
    let p = if do_pad && pad_width > 0 {
        pad_prefix(&p, pad_width, is_template)
    } else {
        p
    };
    colorize(&p, color)
}

/// Format the exit message for a command.
fn format_exit_message(cmd: &CommandInfo) -> String {
    match &cmd.state {
        CommandState::Exited { code } => {
            format!("{} exited with code {}", cmd.command_line, code)
        }
        CommandState::Killed { signal } => {
            format!("{} exited with code {}", cmd.command_line, signal)
        }
        CommandState::Errored { message } => {
            format!("{} errored: {}", cmd.command_line, message)
        }
        _ => String::new(),
    }
}

/// Spawn a single command, sending events back through the channel.
async fn spawn_command(
    index: usize,
    cmd_line: &str,
    tx: mpsc::UnboundedSender<Event>,
    stdin_rx: Option<mpsc::UnboundedReceiver<String>>,
    pipe_stdin: bool,
) {
    let mut cmd = shell_command(cmd_line);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if pipe_stdin {
        cmd.stdin(Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tx.send(Event::Exited {
                index,
                state: CommandState::Errored {
                    message: e.to_string(),
                },
            });
            return;
        }
    };

    let pid = child.id();
    let _ = tx.send(Event::Started { index, pid });

    // Set up stdin forwarding
    if let Some(child_stdin) = child.stdin.take() {
        if let Some(rx) = stdin_rx {
            input::spawn_stdin_writer(child_stdin, rx);
        }
    }

    // Take stdout and stderr for reading
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let tx_out = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx_out.send(Event::Output { index, line }).is_err() {
                break;
            }
        }
    });

    let tx_err = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx_err.send(Event::Output { index, line }).is_err() {
                break;
            }
        }
    });

    // Wait for the process to exit
    let exit_status = child.wait().await;

    // Also wait for output readers to finish flushing
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let state = match exit_status {
        Ok(status) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    CommandState::Killed {
                        signal: signal_name(signal),
                    }
                } else {
                    CommandState::Exited {
                        code: status.code().unwrap_or(1),
                    }
                }
            }
            #[cfg(not(unix))]
            {
                CommandState::Exited {
                    code: status.code().unwrap_or(1),
                }
            }
        }
        Err(e) => CommandState::Errored {
            message: e.to_string(),
        },
    };

    let _ = tx.send(Event::Exited { index, state });
}

/// Convert a Unix signal number to a name.
#[cfg(unix)]
fn signal_name(signal: i32) -> String {
    match signal {
        libc::SIGHUP => "SIGHUP".to_string(),
        libc::SIGINT => "SIGINT".to_string(),
        libc::SIGKILL => "SIGKILL".to_string(),
        libc::SIGTERM => "SIGTERM".to_string(),
        _ => signal.to_string(),
    }
}

/// Parse a signal name (e.g., "SIGTERM", "SIGINT") to its libc constant.
/// Returns SIGTERM as the default for unrecognized signals.
#[cfg(unix)]
fn parse_signal(name: &str) -> i32 {
    match name.to_uppercase().as_str() {
        "SIGHUP" | "HUP" | "1" => libc::SIGHUP,
        "SIGINT" | "INT" | "2" => libc::SIGINT,
        "SIGKILL" | "KILL" | "9" => libc::SIGKILL,
        "SIGTERM" | "TERM" | "15" => libc::SIGTERM,
        _ => libc::SIGTERM,
    }
}

/// Print the timings summary table.
fn print_timings_table(
    commands: &[CommandInfo],
    started_at: &[Option<(Instant, Zoned)>],
    ended_at: &[Option<(Instant, Zoned)>],
) {
    // Calculate column widths
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();

    for (i, cmd) in commands.iter().enumerate() {
        let name = cmd.name.clone();
        let duration = match (&started_at[i], &ended_at[i]) {
            (Some(s), Some(e)) => {
                let ms = e.0.duration_since(s.0).as_millis();
                if ms >= 1000 {
                    format!("{:.2}s", ms as f64 / 1000.0)
                } else {
                    format!("{}ms", ms)
                }
            }
            _ => "?".to_string(),
        };
        let exit_code = cmd.exit_code().map_or("?".to_string(), |c| c.to_string());
        let killed = matches!(cmd.state, CommandState::Killed { .. }).to_string();
        let command = cmd.command_line.clone();

        rows.push((name, duration, exit_code, killed, command));
    }

    // Header
    let headers = ("name", "duration", "exit code", "killed", "command");

    // Calculate column widths
    let w_name = rows
        .iter()
        .map(|r| r.0.len())
        .max()
        .unwrap_or(0)
        .max(headers.0.len());
    let w_dur = rows
        .iter()
        .map(|r| r.1.len())
        .max()
        .unwrap_or(0)
        .max(headers.1.len());
    let w_exit = rows
        .iter()
        .map(|r| r.2.len())
        .max()
        .unwrap_or(0)
        .max(headers.2.len());
    let w_kill = rows
        .iter()
        .map(|r| r.3.len())
        .max()
        .unwrap_or(0)
        .max(headers.3.len());
    let w_cmd = rows
        .iter()
        .map(|r| r.4.len())
        .max()
        .unwrap_or(0)
        .max(headers.4.len());

    let separator = format!(
        "├{:─<w1$}┼{:─<w2$}┼{:─<w3$}┼{:─<w4$}┼{:─<w5$}┤",
        "",
        "",
        "",
        "",
        "",
        w1 = w_name + 2,
        w2 = w_dur + 2,
        w3 = w_exit + 2,
        w4 = w_kill + 2,
        w5 = w_cmd + 2
    );
    let top = format!(
        "┌{:─<w1$}┬{:─<w2$}┬{:─<w3$}┬{:─<w4$}┬{:─<w5$}┐",
        "",
        "",
        "",
        "",
        "",
        w1 = w_name + 2,
        w2 = w_dur + 2,
        w3 = w_exit + 2,
        w4 = w_kill + 2,
        w5 = w_cmd + 2
    );
    let bottom = format!(
        "└{:─<w1$}┴{:─<w2$}┴{:─<w3$}┴{:─<w4$}┴{:─<w5$}┘",
        "",
        "",
        "",
        "",
        "",
        w1 = w_name + 2,
        w2 = w_dur + 2,
        w3 = w_exit + 2,
        w4 = w_kill + 2,
        w5 = w_cmd + 2
    );

    println!("{}", top);
    println!(
        "│ {:w_name$} │ {:w_dur$} │ {:w_exit$} │ {:w_kill$} │ {:w_cmd$} │",
        headers.0, headers.1, headers.2, headers.3, headers.4
    );
    println!("{}", separator);
    for row in &rows {
        println!(
            "│ {:w_name$} │ {:w_dur$} │ {:w_exit$} │ {:w_kill$} │ {:w_cmd$} │",
            row.0, row.1, row.2, row.3, row.4
        );
    }
    println!("{}", bottom);
}

/// Run teardown commands sequentially.
async fn run_teardown_commands(teardown_cmds: &[String]) {
    for cmd_line in teardown_cmds {
        println!(
            "{}",
            format_line("-->", &format!("Running teardown command \"{}\"", cmd_line))
        );

        let mut cmd = shell_command(cmd_line);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                println!(
                    "{}",
                    format_line(
                        "-->",
                        &format!("Teardown command \"{}\" errored: {}", cmd_line, e)
                    )
                );
                continue;
            }
        };

        // Print output from teardown command
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_task = tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                println!("{}", line);
            }
        });

        let stderr_task = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                println!("{}", line);
            }
        });

        let status = child.wait().await;
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);
        println!(
            "{}",
            format_line(
                "-->",
                &format!(
                    "Teardown command \"{}\" exited with code {}",
                    cmd_line, code
                )
            )
        );
    }
}

/// Check if a command should be hidden based on the hide list.
fn should_hide(index: usize, name: &str, hide_list: &[String]) -> bool {
    hide_list
        .iter()
        .any(|h| h == &index.to_string() || h == name)
}

/// Determine the final exit code based on the `--success` strategy.
fn determine_exit_code(
    success: &str,
    commands: &[CommandInfo],
    exit_order: &[(usize, i32)],
) -> i32 {
    match success {
        "first" => exit_order.first().map_or(1, |(_, code)| *code),
        "last" => exit_order.last().map_or(1, |(_, code)| *code),
        "all" => {
            if commands.iter().all(|c| c.exit_code() == Some(0)) {
                0
            } else {
                1
            }
        }
        s => {
            // Handle command-{index}, command-{name}, and their negations
            if let Some(pattern) = s.strip_prefix('!') {
                // Negation: all commands EXCEPT the specified one must succeed
                if let Some(target) = parse_command_pattern(pattern, commands) {
                    // All commands except the target must have exit code 0
                    if commands
                        .iter()
                        .filter(|c| c.index != target)
                        .all(|c| c.exit_code() == Some(0))
                    {
                        0
                    } else {
                        1
                    }
                } else {
                    // Unknown pattern, fall back to "all" behavior
                    if commands.iter().all(|c| c.exit_code() == Some(0)) {
                        0
                    } else {
                        1
                    }
                }
            } else if let Some(target) = parse_command_pattern(s, commands) {
                // The specified command must succeed
                commands
                    .get(target)
                    .and_then(|c| c.exit_code())
                    .unwrap_or(1)
            } else {
                // Unknown pattern, fall back to "all" behavior
                if commands.iter().all(|c| c.exit_code() == Some(0)) {
                    0
                } else {
                    1
                }
            }
        }
    }
}

/// Parse a command pattern like "command-0" or "command-foo" and return the matching command index.
fn parse_command_pattern(pattern: &str, commands: &[CommandInfo]) -> Option<usize> {
    let suffix = pattern.strip_prefix("command-")?;

    // First, try to parse as an index
    if let Ok(index) = suffix.parse::<usize>() {
        if index < commands.len() {
            return Some(index);
        }
    }

    // Otherwise, try to match by name
    commands.iter().find(|c| c.name == suffix).map(|c| c.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_hide() {
        let hide = vec!["1".to_string(), "foo".to_string()];
        assert!(!should_hide(0, "bar", &hide));
        assert!(should_hide(1, "bar", &hide));
        assert!(should_hide(2, "foo", &hide));
        assert!(!should_hide(0, "baz", &[]));
    }

    #[test]
    fn test_should_restart() {
        assert!(!should_restart(0, 0));
        assert!(should_restart(1, 0));
        assert!(!should_restart(1, 1));
        assert!(should_restart(2, 1));
        assert!(should_restart(-1, 100)); // infinite
    }

    #[test]
    fn test_determine_exit_code_all_success() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 0)];
        assert_eq!(determine_exit_code("all", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_all_failure() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        assert_eq!(determine_exit_code("all", &commands, &order), 1);
    }

    #[test]
    fn test_determine_exit_code_first() {
        let commands: Vec<CommandInfo> = vec![];
        let order = vec![(1, 0), (0, 1)];
        assert_eq!(determine_exit_code("first", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_last() {
        let commands: Vec<CommandInfo> = vec![];
        let order = vec![(0, 0), (1, 1)];
        assert_eq!(determine_exit_code("last", &commands, &order), 1);
    }

    #[test]
    fn test_format_exit_message() {
        let mut cmd = CommandInfo::new(0, "0".into(), "echo hello".into());
        cmd.state = CommandState::Exited { code: 0 };
        assert_eq!(format_exit_message(&cmd), "echo hello exited with code 0");

        cmd.state = CommandState::Killed {
            signal: "SIGTERM".to_string(),
        };
        assert_eq!(
            format_exit_message(&cmd),
            "echo hello exited with code SIGTERM"
        );
    }

    #[test]
    fn test_parse_max_processes_none() {
        // None means no limit (use num_commands)
        assert_eq!(parse_max_processes(None, 5), 5);
        assert_eq!(parse_max_processes(None, 10), 10);
    }

    #[test]
    fn test_parse_max_processes_numeric() {
        assert_eq!(parse_max_processes(Some("1"), 5), 1);
        assert_eq!(parse_max_processes(Some("3"), 5), 3);
        assert_eq!(parse_max_processes(Some("10"), 5), 10);
    }

    #[test]
    fn test_parse_max_processes_zero() {
        // 0 means no limit
        assert_eq!(parse_max_processes(Some("0"), 5), 5);
    }

    #[test]
    fn test_parse_max_processes_percentage() {
        // Percentage of available CPUs (at least 1)
        let num_cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        let result = parse_max_processes(Some("50%"), 10);
        let expected = ((num_cpus as f64 * 50.0 / 100.0).round() as usize).max(1);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_max_processes_invalid() {
        // Invalid values default to num_commands
        assert_eq!(parse_max_processes(Some("abc"), 5), 5);
        assert_eq!(parse_max_processes(Some(""), 5), 5);
        assert_eq!(parse_max_processes(Some("abc%"), 5), 5);
    }

    #[test]
    fn test_parse_command_pattern_by_index() {
        let commands = vec![
            CommandInfo::new(0, "foo".into(), "echo foo".into()),
            CommandInfo::new(1, "bar".into(), "echo bar".into()),
        ];
        assert_eq!(parse_command_pattern("command-0", &commands), Some(0));
        assert_eq!(parse_command_pattern("command-1", &commands), Some(1));
        assert_eq!(parse_command_pattern("command-2", &commands), None); // out of bounds
    }

    #[test]
    fn test_parse_command_pattern_by_name() {
        let commands = vec![
            CommandInfo::new(0, "foo".into(), "echo foo".into()),
            CommandInfo::new(1, "bar".into(), "echo bar".into()),
        ];
        assert_eq!(parse_command_pattern("command-foo", &commands), Some(0));
        assert_eq!(parse_command_pattern("command-bar", &commands), Some(1));
        assert_eq!(parse_command_pattern("command-baz", &commands), None); // not found
    }

    #[test]
    fn test_parse_command_pattern_invalid() {
        let commands = vec![CommandInfo::new(0, "foo".into(), "echo".into())];
        assert_eq!(parse_command_pattern("invalid", &commands), None);
        assert_eq!(parse_command_pattern("cmd-0", &commands), None);
        assert_eq!(parse_command_pattern("", &commands), None);
    }

    #[test]
    fn test_determine_exit_code_command_index_success() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // command-0 succeeded, so exit 0
        assert_eq!(determine_exit_code("command-0", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_command_index_failure() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // command-1 failed with code 1
        assert_eq!(determine_exit_code("command-1", &commands, &order), 1);
    }

    #[test]
    fn test_determine_exit_code_command_name_success() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "foo".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "bar".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // command-foo succeeded
        assert_eq!(determine_exit_code("command-foo", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_command_name_failure() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "foo".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "bar".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // command-bar failed
        assert_eq!(determine_exit_code("command-bar", &commands, &order), 1);
    }

    #[test]
    fn test_determine_exit_code_negated_command_index_success() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // All commands except command-1 succeeded (only command-0, which has code 0)
        assert_eq!(determine_exit_code("!command-1", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_negated_command_index_failure() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // All commands except command-0 must succeed, but command-1 failed
        assert_eq!(determine_exit_code("!command-0", &commands, &order), 1);
    }

    #[test]
    fn test_determine_exit_code_negated_command_name() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "foo".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "bar".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // All commands except command-bar (only foo) must succeed
        assert_eq!(determine_exit_code("!command-bar", &commands, &order), 0);
    }

    #[test]
    fn test_determine_exit_code_unknown_pattern_fallback() {
        let commands = vec![
            {
                let mut c = CommandInfo::new(0, "0".into(), "echo".into());
                c.state = CommandState::Exited { code: 0 };
                c
            },
            {
                let mut c = CommandInfo::new(1, "1".into(), "fail".into());
                c.state = CommandState::Exited { code: 1 };
                c
            },
        ];
        let order = vec![(0, 0), (1, 1)];
        // Unknown pattern falls back to "all" behavior
        assert_eq!(determine_exit_code("unknown", &commands, &order), 1);
        assert_eq!(determine_exit_code("command-99", &commands, &order), 1);
        assert_eq!(determine_exit_code("!command-99", &commands, &order), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_signal() {
        // Full names
        assert_eq!(parse_signal("SIGTERM"), libc::SIGTERM);
        assert_eq!(parse_signal("SIGINT"), libc::SIGINT);
        assert_eq!(parse_signal("SIGKILL"), libc::SIGKILL);
        assert_eq!(parse_signal("SIGHUP"), libc::SIGHUP);

        // Short names
        assert_eq!(parse_signal("TERM"), libc::SIGTERM);
        assert_eq!(parse_signal("INT"), libc::SIGINT);
        assert_eq!(parse_signal("KILL"), libc::SIGKILL);
        assert_eq!(parse_signal("HUP"), libc::SIGHUP);

        // Numeric
        assert_eq!(parse_signal("15"), libc::SIGTERM);
        assert_eq!(parse_signal("2"), libc::SIGINT);
        assert_eq!(parse_signal("9"), libc::SIGKILL);
        assert_eq!(parse_signal("1"), libc::SIGHUP);

        // Case insensitive
        assert_eq!(parse_signal("sigterm"), libc::SIGTERM);
        assert_eq!(parse_signal("Sigint"), libc::SIGINT);

        // Unknown defaults to SIGTERM
        assert_eq!(parse_signal("UNKNOWN"), libc::SIGTERM);
        assert_eq!(parse_signal(""), libc::SIGTERM);
    }

    #[test]
    fn test_parse_restart_delay_fixed() {
        assert_eq!(parse_restart_delay("0"), RestartDelay::Fixed(0));
        assert_eq!(parse_restart_delay("100"), RestartDelay::Fixed(100));
        assert_eq!(parse_restart_delay("5000"), RestartDelay::Fixed(5000));
    }

    #[test]
    fn test_parse_restart_delay_exponential() {
        assert_eq!(
            parse_restart_delay("exponential"),
            RestartDelay::Exponential
        );
        assert_eq!(
            parse_restart_delay("EXPONENTIAL"),
            RestartDelay::Exponential
        );
        assert_eq!(
            parse_restart_delay("Exponential"),
            RestartDelay::Exponential
        );
    }

    #[test]
    fn test_parse_restart_delay_invalid() {
        // Invalid values default to 0ms delay
        assert_eq!(parse_restart_delay(""), RestartDelay::Fixed(0));
        assert_eq!(parse_restart_delay("abc"), RestartDelay::Fixed(0));
        assert_eq!(parse_restart_delay("-100"), RestartDelay::Fixed(0));
    }

    #[test]
    fn test_calculate_restart_delay_fixed() {
        let fixed = RestartDelay::Fixed(500);
        assert_eq!(calculate_restart_delay(&fixed, 0), 500);
        assert_eq!(calculate_restart_delay(&fixed, 1), 500);
        assert_eq!(calculate_restart_delay(&fixed, 10), 500);
    }

    #[test]
    fn test_calculate_restart_delay_exponential() {
        let exp = RestartDelay::Exponential;
        assert_eq!(calculate_restart_delay(&exp, 0), 100); // 100 * 2^0 = 100
        assert_eq!(calculate_restart_delay(&exp, 1), 200); // 100 * 2^1 = 200
        assert_eq!(calculate_restart_delay(&exp, 2), 400); // 100 * 2^2 = 400
        assert_eq!(calculate_restart_delay(&exp, 3), 800); // 100 * 2^3 = 800
        assert_eq!(calculate_restart_delay(&exp, 9), 51200); // 100 * 2^9 = 51200 (max)
        assert_eq!(calculate_restart_delay(&exp, 10), 51200); // capped at 2^9
        assert_eq!(calculate_restart_delay(&exp, 100), 51200); // still capped
    }
}
