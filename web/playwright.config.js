const { defineConfig } = require('@playwright/test');
const path = require('node:path');
const executable = path.resolve(__dirname, '../target/debug/omni-workbench' + (process.platform === 'win32' ? '.exe' : ''));
module.exports = defineConfig({
  testDir: './tests', workers: 1, fullyParallel: false, timeout: 45000,
  use: {baseURL:'http://127.0.0.1:19092', viewport:{width:1440,height:1000}, trace:'retain-on-failure'},
  reporter: [['list'], ['html', {open:'never'}]],
  webServer: {command:`"${executable}" --listen 127.0.0.1:19092 --workspace ../target/ui-test-workspace`, url:'http://127.0.0.1:19092/api/status', reuseExistingServer:false, timeout:30000},
});
