import { execSync } from 'node:child_process';
import { createServer } from 'node:net';
import { defineConfig, type Plugin } from 'vitest/config';
import react from '@vitejs/plugin-react';

// Off the usual 5173/1420 range so it doesn't collide with other Vite projects.
const PORT = 41987;
const HOST = '127.0.0.1';

function listeningPids(port: number): number[] {
  const cmd =
    process.platform === 'win32'
      ? `powershell -NoProfile -Command "(Get-NetTCPConnection -LocalPort ${port} -State Listen -ErrorAction SilentlyContinue).OwningProcess"`
      : `lsof -nP -t -iTCP:${port} -sTCP:LISTEN`;
  try {
    const out = execSync(cmd, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
    return [...new Set(out.split(/\s+/).map(Number))].filter((pid) => pid > 0 && pid !== process.pid);
  } catch {
    return []; // lsof exits 1 when nothing listens
  }
}

function portFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = createServer()
      .once('error', () => resolve(false))
      .once('listening', () => probe.close(() => resolve(true)))
      .listen(port, HOST);
  });
}

// Kills whatever already listens on the dev port, so a stale dev server never blocks a restart.
function freePort(): Plugin {
  return {
    name: 'free-port',
    apply: 'serve',
    async configureServer(server) {
      if (process.env.VITEST) return;
      const port = server.config.server.port ?? PORT;
      const pids = listeningPids(port);
      if (pids.length === 0) return;
      for (const pid of pids) {
        try {
          process.kill(pid, 'SIGTERM');
        } catch {}
      }
      server.config.logger.info(`Stopped process ${pids.join(', ')} holding port ${port}`);
      for (let i = 0; i < 30 && !(await portFree(port)); i++) {
        await new Promise((r) => setTimeout(r, 100));
      }
    },
  };
}

export default defineConfig({
  plugins: [react(), freePort()],
  clearScreen: false,
  server: { strictPort: true, port: PORT, host: HOST },
  test: { environment: 'jsdom' },
});
