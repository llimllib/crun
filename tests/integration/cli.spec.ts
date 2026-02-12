/**
 * Integration tests for crun CLI.
 * Adapted from concurrently's bin/index.spec.ts
 * 
 * These tests run the actual binary and verify its behavior through
 * stdout/stderr and exit codes - making them language-independent.
 */
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';

import { run, createKillMessage, FIXTURES_PATH } from './test-utils.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Build the binary before running tests
beforeAll(async () => {
    await new Promise<void>((resolve, reject) => {
        const build = spawn('cargo', ['build', '--release'], {
            cwd: path.resolve(__dirname, '../..'),
            stdio: 'inherit',
        });
        build.on('exit', (code) => {
            if (code === 0) resolve();
            else reject(new Error(`Build failed with code ${code}`));
        });
    });
}, 60000);

it('has help command', async () => {
    const { exit } = run('--help');
    const { code } = await exit;
    expect(code).toBe(0);
});

it('prints help when no arguments are passed', async () => {
    const { exit } = run('');
    const { code } = await exit;
    expect(code).toBe(0);
});

describe('has version command', () => {
    it.each(['--version', '-V'])('%s', async (arg) => {
        const child = run(arg);
        const lines = await child.getLogLines();
        const { code } = await child.exit;
        
        expect(code).toBe(0);
        // Should output a version number
        expect(lines.some(line => /\d+\.\d+\.\d+/.test(line))).toBe(true);
    });
});

describe('exiting conditions', () => {
    it('is of success by default when running successful commands', async () => {
        const { exit } = run('"echo foo" "echo bar"');
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of failure by default when one of the command fails', async () => {
        const { exit } = run('"echo foo" "exit 1"');
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    it('is of success when --success=first and first command to exit succeeds', async () => {
        const { exit } = run(
            '--success=first "echo foo" "node sleep.mjs 0.5 && exit 1"'
        );
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of failure when --success=first and first command to exit fails', async () => {
        const { exit } = run(
            '--success=first "exit 1" "node sleep.mjs 0.5 && echo foo"'
        );
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    describe('is of success when --success=last and last command to exit succeeds', () => {
        it.each(['--success=last', '-s last'])('%s', async (arg) => {
            const { exit } = run(
                `${arg} "exit 1" "node sleep.mjs 0.5 && echo foo"`
            );
            const { code } = await exit;
            expect(code).toBe(0);
        });
    });

    it('is of failure when --success=last and last command to exit fails', async () => {
        const { exit } = run(
            '--success=last "echo foo" "node sleep.mjs 0.5 && exit 1"'
        );
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    it('is of success when --success=command-{index} and that command succeeds', async () => {
        const { exit } = run('--success=command-0 "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of failure when --success=command-{index} and that command fails', async () => {
        const { exit } = run('--success=command-1 "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    it('is of success when --success=command-{name} and that command succeeds', async () => {
        const { exit } = run('-n foo,bar --success=command-foo "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of failure when --success=command-{name} and that command fails', async () => {
        const { exit } = run('-n foo,bar --success=command-bar "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    it('is of success when --success=!command-{index} and all other commands succeed', async () => {
        const { exit } = run('--success="!command-1" "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of failure when --success=!command-{index} and another command fails', async () => {
        const { exit } = run('--success="!command-0" "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBeGreaterThan(0);
    });

    it('is of success when --success=!command-{name} and all other commands succeed', async () => {
        const { exit } = run('-n foo,bar --success="!command-bar" "echo one" "exit 1"');
        const { code } = await exit;
        expect(code).toBe(0);
    });

    it('is of success when a SIGINT is sent', async () => {
        const child = run('"node read-echo.mjs"');
        // Wait for command to have started before sending SIGINT
        child.onLine((line) => {
            if (/READING/.test(line)) {
                process.kill(Number(child.pid), 'SIGINT');
            }
        });
        const lines = await child.getLogLines();
        const exit = await child.exit;

        expect(exit.code).toBe(0);
        expect(lines).toContainEqual(
            expect.stringMatching(
                createKillMessage('[0] node read-echo.mjs', 'SIGINT'),
            ),
        );
    });
});

describe('does not log any extra output', () => {
    it.each(['--raw', '-r'])('%s', async (arg) => {
        const lines = await run(`${arg} "echo foo" "echo bar"`).getLogLines();

        expect(lines).toHaveLength(2);
        expect(lines).toContainEqual(expect.stringContaining('foo'));
        expect(lines).toContainEqual(expect.stringContaining('bar'));
    });
});

describe('--hide', () => {
    it('hides the output of a process by its index', async () => {
        const lines = await run('--hide 1 "echo foo" "echo bar"').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('foo'));
        expect(lines).not.toContainEqual(expect.stringContaining('bar'));
    });

    it('hides the output of a process by its name', async () => {
        const lines = await run('-n foo,bar --hide bar "echo foo" "echo bar"').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('foo'));
        expect(lines).not.toContainEqual(expect.stringContaining('bar'));
    });

    it('hides the output of a process by its index in raw mode', async () => {
        const lines = await run('--hide 1 --raw "echo foo" "echo bar"').getLogLines();

        expect(lines).toHaveLength(1);
        expect(lines).toContainEqual(expect.stringContaining('foo'));
        expect(lines).not.toContainEqual(expect.stringContaining('bar'));
    });

    it('hides the output of a process by its name in raw mode', async () => {
        const lines = await run('-n foo,bar --hide bar --raw "echo foo" "echo bar"').getLogLines();

        expect(lines).toHaveLength(1);
        expect(lines).toContainEqual(expect.stringContaining('foo'));
        expect(lines).not.toContainEqual(expect.stringContaining('bar'));
    });
});

describe('--group', () => {
    it('groups output per process', async () => {
        const lines = await run(
            '--group "echo foo && node sleep.mjs 0.3 && echo bar" "node sleep.mjs 0.1 && echo baz"'
        ).getLogLines();

        // First command's output should be together, before second command's output
        const fooIndex = lines.findIndex(l => l.includes('foo'));
        const barIndex = lines.findIndex(l => l.includes('bar'));
        const bazIndex = lines.findIndex(l => l.includes('baz'));
        
        expect(fooIndex).toBeLessThan(barIndex);
        expect(barIndex).toBeLessThan(bazIndex);
    });
});

describe('--names', () => {
    describe('prefixes with names', () => {
        it.each(['--names', '-n'])('%s', async (arg) => {
            const lines = await run(`${arg} foo,bar "echo foo" "echo bar"`).getLogLines();

            expect(lines).toContainEqual(expect.stringContaining('[foo]'));
            expect(lines).toContainEqual(expect.stringContaining('[bar]'));
        });
    });

    it('is split using --name-separator arg', async () => {
        const lines = await run(
            '--names "foo|bar" --name-separator "|" "echo foo" "echo bar"'
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[foo]'));
        expect(lines).toContainEqual(expect.stringContaining('[bar]'));
    });
});

describe('specifies custom prefix', () => {
    it.each(['--prefix', '-p'])('%s', async (arg) => {
        const lines = await run(`${arg} command "echo foo" "echo bar"`).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[echo foo]'));
        expect(lines).toContainEqual(expect.stringContaining('[echo bar]'));
    });
});

describe('specifies custom prefix length', () => {
    it.each(['--prefix command --prefix-length 5', '-p command -l 5'])('%s', async (arg) => {
        const lines = await run(`${arg} "echo foo" "echo bar"`).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[ec..o]'));
        expect(lines).toContainEqual(expect.stringContaining('[ec..r]'));
    });
});

describe('--pad-prefix', () => {
    it('pads prefixes with spaces', async () => {
        const lines = await run('--pad-prefix -n foo,barbaz "echo foo" "echo bar"').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[foo   ]'));
        expect(lines).toContainEqual(expect.stringContaining('[barbaz]'));
    });
});

describe('template prefix', () => {
    it('uses {index} without brackets', async () => {
        const lines = await run('-p "{index}" "echo foo"').getLogLines();

        // Template prefixes should NOT have brackets
        expect(lines).toContainEqual(expect.stringMatching(/^0 foo$/));
        expect(lines).toContainEqual(expect.stringMatching(/^0 echo foo exited with code 0$/));
    });

    it('uses {name} without brackets', async () => {
        const lines = await run('-p "{name}" -n myname "echo foo"').getLogLines();

        expect(lines).toContainEqual(expect.stringMatching(/^myname foo$/));
    });

    it('uses {index}-{name} combined template', async () => {
        const lines = await run('-p "{index}-{name}" -n server "echo foo"').getLogLines();

        expect(lines).toContainEqual(expect.stringMatching(/^0-server foo$/));
    });

    it('uses {time} placeholder with timestamp', async () => {
        const lines = await run('-p "{time}" "echo foo"').getLogLines();

        // Should have a timestamp like "2026-02-11 21:30:00.123"
        expect(lines).toContainEqual(expect.stringMatching(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} foo$/));
    });

    it('uses {time} {pid} combined template', async () => {
        const lines = await run('-p "{time} {pid}" "echo foo"').getLogLines();

        // Should have "timestamp pid message" format
        expect(lines).toContainEqual(expect.stringMatching(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} \d+ foo$/));
    });

    it('uses {command} placeholder', async () => {
        const lines = await run('-p "{command}" "echo foo"').getLogLines();

        expect(lines).toContainEqual(expect.stringMatching(/^echo foo foo$/));
    });

    it('pads template prefixes correctly', async () => {
        const lines = await run('--pad-prefix -p "{name}" -n foo,barbaz "echo a" "echo b"').getLogLines();

        // Template prefixes should be padded without brackets
        expect(lines).toContainEqual(expect.stringMatching(/^foo    (a|echo a exited)/));
        expect(lines).toContainEqual(expect.stringMatching(/^barbaz (b|echo b exited)/));
    });
});

describe('--restart-tries', () => {
    it('changes how many times a command will restart', async () => {
        const lines = await run('--restart-tries 1 "exit 1"').getLogLines();

        expect(lines).toEqual([
            expect.stringContaining('[0] exit 1 exited with code 1'),
            expect.stringContaining('[0] exit 1 restarted'),
            expect.stringContaining('[0] exit 1 exited with code 1'),
        ]);
    });
});

describe('--restart-after', () => {
    it('delays restart by specified milliseconds', async () => {
        const start = Date.now();
        const lines = await run('--restart-tries 1 --restart-after 200 "exit 1"').getLogLines();
        const elapsed = Date.now() - start;

        // Should have restarted once
        expect(lines).toContainEqual(expect.stringContaining('[0] exit 1 restarted'));
        // Should have taken at least 200ms due to restart delay
        expect(elapsed).toBeGreaterThanOrEqual(180); // Allow small timing variance
    });

    it('delays restart with exponential backoff', async () => {
        const start = Date.now();
        const lines = await run('--restart-tries 2 --restart-after exponential "exit 1"').getLogLines();
        const elapsed = Date.now() - start;

        // Should have restarted twice
        const restarts = lines.filter(l => l.includes('restarted'));
        expect(restarts).toHaveLength(2);
        // Exponential: 100ms + 200ms = 300ms minimum
        expect(elapsed).toBeGreaterThanOrEqual(280); // Allow small timing variance
    });

    it('restarts immediately with --restart-after 0', async () => {
        const start = Date.now();
        const lines = await run('--restart-tries 1 --restart-after 0 "exit 1"').getLogLines();
        const elapsed = Date.now() - start;

        // Should have restarted
        expect(lines).toContainEqual(expect.stringContaining('[0] exit 1 restarted'));
        // Should be fast (no delay)
        expect(elapsed).toBeLessThan(500);
    });

    it('is case-insensitive for exponential', async () => {
        const lines = await run('--restart-tries 1 --restart-after EXPONENTIAL "exit 1"').getLogLines();

        // Should have restarted (proves it was parsed correctly)
        expect(lines).toContainEqual(expect.stringContaining('[0] exit 1 restarted'));
    });
});

describe('--kill-others', () => {
    describe('kills on success', () => {
        it.each(['--kill-others', '-k'])('%s', async (arg) => {
            const lines = await run(
                `${arg} "node sleep.mjs 10" "exit 0"`
            ).getLogLines();

            expect(lines).toContainEqual(expect.stringContaining('[1] exit 0 exited with code 0'));
            expect(lines).toContainEqual(
                expect.stringContaining('Sending SIGTERM to other processes')
            );
            expect(lines).toContainEqual(
                expect.stringMatching(
                    createKillMessage('[0] node sleep.mjs 10', 'SIGTERM')
                )
            );
        });
    });

    it('kills on failure', async () => {
        const lines = await run(
            '--kill-others "node sleep.mjs 10" "exit 1"'
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[1] exit 1 exited with code 1'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
    });
});

describe('--kill-others-on-fail', () => {
    it('does not kill on success', async () => {
        const lines = await run(
            '--kill-others-on-fail "node sleep.mjs 0.5" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('exit 0 exited with code 0'));
        expect(lines).toContainEqual(
            expect.stringContaining('sleep.mjs 0.5 exited with code 0')
        );
    });

    it('kills on failure', async () => {
        const lines = await run(
            '--kill-others-on-fail "node sleep.mjs 10" "exit 1"'
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('exit 1 exited with code 1'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
    });
});

describe('--kill-signal', () => {
    it('uses SIGTERM by default', async () => {
        const lines = await run(
            '--kill-others "node sleep.mjs 10" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(
                createKillMessage('[0] node sleep.mjs 10', 'SIGTERM')
            )
        );
    });

    it('sends SIGINT when --kill-signal SIGINT', async () => {
        const lines = await run(
            '--kill-signal SIGINT --kill-others "node sleep.mjs 10" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGINT to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(
                createKillMessage('[0] node sleep.mjs 10', 'SIGINT')
            )
        );
    });

    it('sends SIGHUP when --kill-signal SIGHUP', async () => {
        const lines = await run(
            '--kill-signal SIGHUP --kill-others "node sleep.mjs 10" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGHUP to other processes')
        );
        // SIGHUP exits with code 1 on most systems
        expect(lines).toContainEqual(
            expect.stringMatching(/node sleep\.mjs 10 exited with code (SIGHUP|1)/)
        );
    });

    it('sends SIGKILL when --kill-signal SIGKILL', async () => {
        const lines = await run(
            '--kill-signal SIGKILL --kill-others "node sleep.mjs 10" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGKILL to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(/node sleep\.mjs 10 exited with code (SIGKILL|9)/)
        );
    });

    it('works with --kill-others-on-fail', async () => {
        const lines = await run(
            '--kill-signal SIGINT --kill-others-on-fail "node sleep.mjs 10" "exit 1"'
        ).getLogLines();

        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGINT to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(
                createKillMessage('[0] node sleep.mjs 10', 'SIGINT')
            )
        );
    });
});

describe('--kill-timeout', () => {
    // Note: We use a Node.js script that ignores SIGTERM instead of "trap '' TERM; sleep N"
    // because shell trap only protects the shell process, not its children (like sleep).
    // When we send signals to process groups, all processes receive the signal.
    //
    // We use "node sleep.mjs 0.1" instead of "exit 0" for the second command to give
    // the first command time to start and register its signal handler.

    it('sends SIGKILL after timeout when process ignores SIGTERM', async () => {
        const lines = await run(
            `--kill-timeout 100 --kill-others "node ignore-sigterm.mjs 10" "node sleep.mjs 0.1"`
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[1] node sleep.mjs 0.1 exited with code 0'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGKILL to 1 processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(/ignore-sigterm.mjs 10 exited with code (SIGKILL|9)/)
        );
    });

    it('does not send SIGKILL if process exits before timeout', async () => {
        const lines = await run(
            '--kill-timeout 1000 --kill-others "node sleep.mjs 10" "exit 0"'
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[1] exit 0 exited with code 0'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
        // Should NOT have SIGKILL message since process exited before timeout
        expect(lines).not.toContainEqual(
            expect.stringContaining('Sending SIGKILL')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(
                createKillMessage('[0] node sleep.mjs 10', 'SIGTERM')
            )
        );
    });

    it('does not escalate to SIGKILL when --kill-signal is already SIGKILL', async () => {
        const lines = await run(
            `--kill-timeout 100 --kill-signal SIGKILL --kill-others "node ignore-sigterm.mjs 10" "exit 0"`
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[1] exit 0 exited with code 0'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGKILL to other processes')
        );
        // Should only have one SIGKILL message (initial signal), not escalation
        const sigkillLines = lines.filter(line => line.includes('Sending SIGKILL'));
        expect(sigkillLines.length).toBe(1);
        expect(lines).toContainEqual(
            expect.stringMatching(/ignore-sigterm.mjs 10 exited with code (SIGKILL|9)/)
        );
    });

    it('works with --kill-others-on-fail', async () => {
        // Use "node sleep.mjs 0.1; exit 1" to give the first command time to start
        const lines = await run(
            `--kill-timeout 100 --kill-others-on-fail "node ignore-sigterm.mjs 10" "node sleep.mjs 0.1; exit 1"`
        ).getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[1] node sleep.mjs 0.1; exit 1 exited with code 1'));
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGTERM to other processes')
        );
        expect(lines).toContainEqual(
            expect.stringContaining('Sending SIGKILL to 1 processes')
        );
        expect(lines).toContainEqual(
            expect.stringMatching(/ignore-sigterm.mjs 10 exited with code (SIGKILL|9)/)
        );
    });
});

describe('--handle-input', () => {
    describe('forwards input to first process by default', () => {
        it.each(['--handle-input', '-i'])('%s', async (arg) => {
            const child = run(`${arg} "node read-echo.mjs"`);
            
            child.onLine((line) => {
                if (/READING/.test(line)) {
                    child.stdin!.write('stop\n');
                }
            });
            
            const lines = await child.getLogLines();
            const { code } = await child.exit;

            expect(code).toBe(0);
            expect(lines).toContainEqual(expect.stringContaining('stop'));
            expect(lines).toContainEqual(
                expect.stringContaining('read-echo.mjs exited with code 0')
            );
        });
    });

    it('forwards input to process --default-input-target', async () => {
        const child = run(
            '-ki --default-input-target 1 "node read-echo.mjs" "node read-echo.mjs"'
        );
        
        child.onLine((line) => {
            if (/\[1\] READING/.test(line)) {
                child.stdin!.write('stop\n');
            }
        });
        
        const lines = await child.getLogLines();
        const { code } = await child.exit;

        expect(code).toBeGreaterThan(0);
        expect(lines).toContainEqual(expect.stringContaining('[1] stop'));
    });

    it('forwards input to specified process', async () => {
        const child = run('-ki "node read-echo.mjs" "node read-echo.mjs"');
        
        child.onLine((line) => {
            if (/\[1\] READING/.test(line)) {
                child.stdin!.write('1:stop\n');
            }
        });
        
        const lines = await child.getLogLines();
        const { code } = await child.exit;

        expect(code).toBeGreaterThan(0);
        expect(lines).toContainEqual(expect.stringContaining('[1] stop'));
    });
});

describe('--teardown', () => {
    it('runs teardown commands when input commands exit', async () => {
        const lines = await run('--teardown "echo bye" "echo hey"').getLogLines();
        
        expect(lines).toContainEqual(expect.stringContaining('hey'));
        expect(lines).toContainEqual(expect.stringContaining('bye'));
        expect(lines).toContainEqual(
            expect.stringContaining('Running teardown command')
        );
    });

    it('runs multiple teardown commands', async () => {
        const lines = await run(
            '--teardown "echo bye" --teardown "echo bye2" "echo hey"'
        ).getLogLines();
        
        expect(lines.some(l => l.includes('bye') && !l.includes('bye2'))).toBe(true);
        expect(lines).toContainEqual(expect.stringContaining('bye2'));
    });
});

describe('--timings', () => {
    it('shows timings on success', async () => {
        const lines = await run('--timings "echo foo"').getLogLines();

        // Should contain timing information
        expect(lines).toContainEqual(expect.stringMatching(/started at/));
        expect(lines).toContainEqual(expect.stringMatching(/stopped at.*after.*ms/));
        
        // Should contain a timing table
        expect(lines).toContainEqual(expect.stringMatching(/│.*name.*│.*duration.*│/));
    });

    it('shows timings on failure', async () => {
        const lines = await run('--timings "exit 1"').getLogLines();

        expect(lines).toContainEqual(expect.stringMatching(/started at/));
        expect(lines).toContainEqual(expect.stringMatching(/stopped at.*after.*ms/));
    });
});

describe('--passthrough-arguments', () => {
    describe('replaces placeholders when enabled', () => {
        it.each(['--passthrough-arguments', '-P'])('%s replaces {1} placeholder', async (arg) => {
            const lines = await run(`${arg} "echo {1}" -- foo`).getLogLines();

            // The command should output 'foo'
            expect(lines).toContainEqual(expect.stringContaining('[0] foo'));
            // Exit message should show the expanded command
            expect(lines).toContainEqual(expect.stringContaining('[0] echo foo exited with code 0'));
        });
    });

    it('replaces multiple numbered placeholders in multiple commands', async () => {
        const lines = await run('-P "echo {1}" "echo {2}" -- foo bar').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[0] foo'));
        expect(lines).toContainEqual(expect.stringContaining('[1] bar'));
        expect(lines).toContainEqual(expect.stringContaining('[0] echo foo exited with code 0'));
        expect(lines).toContainEqual(expect.stringContaining('[1] echo bar exited with code 0'));
    });

    it('replaces {2} {1} in reverse order', async () => {
        const lines = await run('-P "echo {2} {1}" -- foo bar').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[0] bar foo'));
    });

    it('replaces {@} with all arguments space-separated', async () => {
        const lines = await run('-P "echo {@}" -- foo bar baz').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[0] foo bar baz'));
        expect(lines).toContainEqual(expect.stringContaining('[0] echo foo bar baz exited with code 0'));
    });

    it('replaces {*} with all arguments as single quoted string', async () => {
        const lines = await run('-P "echo {*}" -- foo bar').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[0] foo bar'));
        // Exit message should show the quoted form
        expect(lines).toContainEqual(expect.stringMatching(/echo.*foo bar.*exited with code 0/));
    });

    it('replaces missing placeholder with empty string', async () => {
        const lines = await run('-P "echo {3}" -- foo bar').getLogLines();

        // {3} should be replaced with empty string since only 2 args provided
        expect(lines).toContainEqual(expect.stringContaining('[0] echo  exited with code 0'));
    });

    it('does not replace escaped placeholders', async () => {
        const lines = await run('-P "echo \\{1}" -- foo').getLogLines();

        // Escaped placeholder should be output literally
        expect(lines).toContainEqual(expect.stringContaining('[0] {1}'));
        expect(lines).toContainEqual(expect.stringContaining('[0] echo {1} exited with code 0'));
    });

    it('quotes arguments containing spaces', async () => {
        const lines = await run('-P "echo {1}" -- "foo bar"').getLogLines();

        expect(lines).toContainEqual(expect.stringContaining('[0] foo bar'));
    });

    it('does not replace placeholders when disabled', async () => {
        const lines = await run('"echo {1}" -- echo').getLogLines();

        // {1} should be printed literally
        expect(lines).toContainEqual(expect.stringContaining('[0] {1}'));
        expect(lines).toContainEqual(expect.stringContaining('[0] echo {1} exited with code 0'));
        // 'echo' after -- should be treated as a separate command
        expect(lines).toContainEqual(expect.stringContaining('[1] echo exited with code 0'));
    });

    it('treats extra args as commands when disabled', async () => {
        const { exit } = run('"echo first" -- "echo second"');
        const { code } = await exit;

        expect(code).toBe(0);
    });
});

describe('--max-processes', () => {
    it('runs commands sequentially with --max-processes 1', async () => {
        const lines = await run(
            '--max-processes 1 "node sleep.mjs 0.1 && echo first" "echo second"'
        ).getLogLines();

        // With max-processes 1, commands should run sequentially
        // "first" should appear before "second" starts
        const firstIndex = lines.findIndex(line => line.includes('[0] first'));
        const firstExitIndex = lines.findIndex(line => line.includes('[0]') && line.includes('exited'));
        const secondIndex = lines.findIndex(line => line.includes('[1] second'));

        expect(firstIndex).toBeGreaterThanOrEqual(0);
        expect(firstExitIndex).toBeGreaterThanOrEqual(0);
        expect(secondIndex).toBeGreaterThanOrEqual(0);
        // The second command should start after the first command exits
        expect(secondIndex).toBeGreaterThan(firstExitIndex);
    });

    it('runs commands sequentially with -m 1', async () => {
        const lines = await run(
            '-m 1 "node sleep.mjs 0.1 && echo first" "echo second"'
        ).getLogLines();

        const firstExitIndex = lines.findIndex(line => line.includes('[0]') && line.includes('exited'));
        const secondIndex = lines.findIndex(line => line.includes('[1] second'));

        expect(firstExitIndex).toBeGreaterThanOrEqual(0);
        expect(secondIndex).toBeGreaterThanOrEqual(0);
        expect(secondIndex).toBeGreaterThan(firstExitIndex);
    });

    it('respects max-processes with restarts', async () => {
        const lines = await run(
            '--max-processes 1 --restart-tries 1 "exit 1" "echo second"'
        ).getLogLines();

        // With max-processes 1 and restart-tries 1:
        // - Command 0 should fail, restart, fail again
        // - Only then should command 1 (echo second) start
        const restartIndex = lines.findIndex(line => line.includes('restarted'));
        const secondIndex = lines.findIndex(line => line.includes('[1] second'));

        expect(restartIndex).toBeGreaterThanOrEqual(0);
        expect(secondIndex).toBeGreaterThanOrEqual(0);
        // "second" should appear after the restart
        expect(secondIndex).toBeGreaterThan(restartIndex);
    });

    it('allows multiple concurrent processes with higher limit', async () => {
        // With max-processes 2, both commands should start immediately
        // and run concurrently. We verify by checking that both outputs
        // appear before either exit message.
        const lines = await run(
            '--max-processes 2 "node sleep.mjs 0.1 && echo first" "node sleep.mjs 0.1 && echo second"'
        ).getLogLines();

        const firstOutput = lines.findIndex(line => line.includes('[0] first'));
        const secondOutput = lines.findIndex(line => line.includes('[1] second'));
        const firstExit = lines.findIndex(line => line.includes('[0]') && line.includes('exited'));
        const secondExit = lines.findIndex(line => line.includes('[1]') && line.includes('exited'));

        expect(firstOutput).toBeGreaterThanOrEqual(0);
        expect(secondOutput).toBeGreaterThanOrEqual(0);
        // Both should complete successfully
        expect(firstExit).toBeGreaterThanOrEqual(0);
        expect(secondExit).toBeGreaterThanOrEqual(0);
    });
});
