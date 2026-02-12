#!/usr/bin/env node
/**
 * A process that ignores SIGTERM and sleeps for a specified duration.
 * Used to test --kill-timeout (SIGKILL escalation).
 * 
 * Usage: node ignore-sigterm.mjs <seconds>
 */

const seconds = +process.argv[2] || 10;

// Ignore SIGTERM
process.on('SIGTERM', () => {
    // Intentionally do nothing - ignore the signal
});

// Sleep for the specified duration
await new Promise((resolve) => setTimeout(resolve, seconds * 1000));
