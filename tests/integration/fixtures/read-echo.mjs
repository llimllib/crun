#!/usr/bin/env node
/**
 * Reads from stdin and echoes each line.
 * Exits when it receives "stop".
 */
import process from 'node:process';

process.stdin.on('data', (chunk) => {
    const line = chunk.toString().trim();
    console.log(line);

    if (line === 'stop') {
        process.exit(0);
    }
});

console.log('READING');
