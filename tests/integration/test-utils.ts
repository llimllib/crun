/**
 * Test runner utilities for crun integration tests.
 * Adapted from concurrently's bin/index.spec.ts
 */
import { spawn, ChildProcess } from 'node:child_process';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Path to the built binary
const BINARY_PATH = path.resolve(__dirname, '../../target/release/crun');

// Path to fixtures
export const FIXTURES_PATH = path.resolve(__dirname, 'fixtures');

const isWindows = process.platform === 'win32';

/**
 * Creates a regex pattern for matching exit messages across platforms.
 */
export const createKillMessage = (prefix: string, signal: 'SIGTERM' | 'SIGINT' | string) => {
    const map: Record<string, string | number> = {
        SIGTERM: isWindows ? '1' : '(SIGTERM|143)',
        SIGINT: isWindows ? '(3221225786|0)' : '(SIGINT|130|0)',
    };
    return new RegExp(`${escapeRegExp(prefix)} exited with code ${map[signal] ?? signal}`);
};

/**
 * Escapes special regex characters in a string.
 */
export const escapeRegExp = (str: string) => {
    return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
};

export interface RunResult {
    /** The spawned child process */
    process: ChildProcess;
    /** Stdin stream for writing input */
    stdin: ChildProcess['stdin'];
    /** Process ID */
    pid: number | undefined;
    /** Promise that resolves with exit info when the process exits */
    exit: Promise<{ code: number | null; signal: NodeJS.Signals | null }>;
    /** Get all output lines after the process completes */
    getLogLines: () => Promise<string[]>;
    /** Subscribe to output lines as they come */
    onLine: (callback: (line: string) => void) => void;
}

/**
 * Runs crun with the given arguments.
 * Returns observables/promises for the output and exit status.
 */
export const run = (args: string): RunResult => {
    // Parse args string into array (simple split, doesn't handle all quoting)
    const argArray = parseArgs(args);
    
    const child = spawn(BINARY_PATH, argArray, {
        cwd: FIXTURES_PATH,
        env: {
            ...process.env,
            // Disable color output for consistent test results
            NO_COLOR: '1',
        },
    });

    const lines: string[] = [];
    const lineCallbacks: ((line: string) => void)[] = [];

    const stdout = readline.createInterface({ input: child.stdout! });
    const stderr = readline.createInterface({ input: child.stderr! });

    const handleLine = (line: string) => {
        lines.push(line);
        lineCallbacks.forEach(cb => cb(line));
    };

    stdout.on('line', handleLine);
    stderr.on('line', handleLine);

    const exit = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => {
        child.on('exit', (code, signal) => {
            resolve({ code, signal });
        });
    });

    const getLogLines = async (): Promise<string[]> => {
        await exit;
        // Small delay to ensure all output is captured
        await new Promise(resolve => setTimeout(resolve, 50));
        return [...lines];
    };

    const onLine = (callback: (line: string) => void) => {
        lineCallbacks.push(callback);
    };

    return {
        process: child,
        stdin: child.stdin,
        pid: child.pid,
        exit,
        getLogLines,
        onLine,
    };
};

/**
 * Simple argument parser that handles basic quoting.
 * Converts a string like: --flag "arg with spaces" 'another arg'
 * into: ['--flag', 'arg with spaces', 'another arg']
 */
function parseArgs(input: string): string[] {
    const args: string[] = [];
    let current = '';
    let inQuote: string | null = null;
    
    for (let i = 0; i < input.length; i++) {
        const char = input[i];
        
        if (inQuote) {
            if (char === inQuote) {
                inQuote = null;
            } else {
                current += char;
            }
        } else if (char === '"' || char === "'") {
            inQuote = char;
        } else if (char === ' ' || char === '\t') {
            if (current) {
                args.push(current);
                current = '';
            }
        } else {
            current += char;
        }
    }
    
    if (current) {
        args.push(current);
    }
    
    return args;
}
