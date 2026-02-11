#!/usr/bin/env node
/**
 * Platform independent implementation of 'sleep' used as a command in tests.
 * (Windows doesn't provide the 'sleep' command by default)
 */
const seconds = +process.argv[2];
if (!seconds || Number.isNaN(seconds) || process.argv.length > 3) {
    console.error('usage: sleep seconds');
    process.exit(1);
}

await new Promise((resolve) => setTimeout(resolve, seconds * 1000));
