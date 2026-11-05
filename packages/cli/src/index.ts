#!/usr/bin/env node
const [command] = process.argv.slice(2);
if (command === 'health') { const response = await fetch(process.env.OMNISTREAM_URL ?? 'http://127.0.0.1:4317/health'); console.log(await response.text()); }
else console.log('omnistream commands: health');
