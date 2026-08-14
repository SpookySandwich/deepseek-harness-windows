// bridge.mjs — subscribes to the dsh mux WebSocket downlink and emits
// turn-completion events as JSON lines on stdout for the Tauri shell.
//
// Uses only Node built-ins (global WebSocket); no npm dependencies.

const port = process.env.DSH_PORT;
if (!port) {
  process.stderr.write('bridge: DSH_PORT not set\n');
  process.exit(1);
}

let attempt = 0;

function connect() {
  let scheduled = false;
  const scheduleReconnect = () => {
    if (scheduled) return;
    scheduled = true;
    attempt += 1;
    const backoff = Math.min(5000, 500 * attempt);
    setTimeout(connect, backoff);
  };

  let ws;
  try {
    ws = new WebSocket(`ws://127.0.0.1:${port}/api/events.mux`);
  } catch {
    scheduleReconnect();
    return;
  }

  ws.onopen = () => { attempt = 0; };
  ws.onmessage = (event) => {
    let message;
    try { message = JSON.parse(event.data); } catch { return; }
    if (message && message.method === 'session/event') {
      const sessionEvent = message.payload && message.payload.event;
      if (sessionEvent && sessionEvent.type === 'turn/end') {
        process.stdout.write(JSON.stringify({ type: 'turn-end' }) + '\n');
      }
    }
  };
  ws.onerror = () => { scheduleReconnect(); };
  ws.onclose = () => { scheduleReconnect(); };
}

connect();
