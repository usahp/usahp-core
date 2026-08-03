import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';

type Phase = 'stopped' | 'starting' | 'running' | 'stopping' | 'error';
type Outcome = 'accepted' | { rejected: { reason: string } };
type Connection = {
  client_id: number;
  peer: string | null;
  app_id: string | null;
  pid: number | null;
  requested_mode: string | null;
  outcome: Outcome | null;
};
type Session = {
  client_id: number;
  app_id: string;
  pid: number | null;
  requested_mode: string;
  session_id: string;
};
type CaptureStatus = {
  active: boolean;
  availability: 'available' | 'permission_required' | 'unavailable';
  message: string | null;
};
type Snapshot = {
  configured: boolean;
  phase: Phase;
  config_path: string | null;
  address: string | null;
  capture: CaptureStatus;
  switches: Array<{ switch_id: string; state: string; confidence?: number }>;
  connections: Connection[];
  active_session: Session | null;
  error: string | null;
};

const app = document.querySelector<HTMLDivElement>('#app')!;
app.innerHTML = `
  <header><div class="brand"><span class="mark" aria-hidden="true"></span><div><h1>USAHP Control</h1><p>Local switch service</p></div></div><span id="status" class="badge">Loading</span></header>
  <main>
    <section class="hero card"><div><p class="eyebrow">Service</p><h2 id="phase">Loading…</h2><p id="summary">Reading service state.</p></div><div class="actions"><button id="start">Start service</button><button id="stop" class="secondary">Stop service</button></div></section>
    <section id="permission-card" class="warning card" hidden><strong>macOS Accessibility permission required</strong><p>USAHP cannot capture or suppress configured keys until access is granted. macOS may ask you to enable USAHP Control in System Settings.</p><button id="grant-permission">Grant Accessibility Access</button></section>
    <section id="error-card" class="error card" hidden><strong>Service error</strong><p id="error"></p></section>
    <div class="grid">
      <section class="card"><div class="section-title"><h2>Configuration</h2><button id="config" class="text">Choose file</button></div><dl><dt>File</dt><dd id="config-path">Not selected</dd><dt>WebSocket</dt><dd id="address">—</dd><dt>Capture</dt><dd id="capture">—</dd></dl><p class="notice">On first launch, USAHP creates and starts a default configuration that globally captures and suppresses Space and Enter while the service is running.</p></section>
      <section class="card"><h2>Active owner</h2><div id="session" class="empty">No managed session</div></section>
    </div>
    <section class="card"><div class="section-title"><h2>Connected applications</h2><span id="connection-count" class="count">0</span></div><div id="connections" class="empty">No clients connected</div></section>
    <section class="card"><div class="section-title"><h2>Logical switches</h2><span id="switch-count" class="count">0</span></div><div id="switches" class="switches empty">No switches loaded</div></section>
  </main>
  <footer><span>Live state only — nothing is retained after disconnect.</span><button id="quit" class="danger text">Quit USAHP</button></footer>
  <dialog id="confirm"><form method="dialog"><h2>Active app will lose switch control</h2><p id="confirm-copy"></p><div class="actions"><button value="cancel" class="secondary">Cancel</button><button id="confirm-action" value="confirm" class="danger">Continue</button></div></form></dialog>
`;

let current: Snapshot | null = null;
let pendingAction: 'stop' | 'quit' | null = null;
const $ = <T extends Element>(selector: string) => document.querySelector<T>(selector)!;

function text(value: unknown): string {
  return String(value).replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]!);
}

function outcome(value: Outcome | null): string {
  if (!value) return 'No request';
  if (value === 'accepted') return 'Accepted';
  return `Rejected — ${value.rejected.reason}`;
}

function render(snapshot: Snapshot) {
  current = snapshot;
  const labels: Record<Phase, string> = { stopped: 'Stopped', starting: 'Starting…', running: 'Running', stopping: 'Stopping…', error: 'Needs attention' };
  $('#status').textContent = labels[snapshot.phase];
  $('#status').className = `badge ${snapshot.phase}`;
  $('#phase').textContent = labels[snapshot.phase];
  $('#summary').textContent = snapshot.configured ? (snapshot.phase === 'running' ? `Listening on ${snapshot.address}` : 'The tray utility remains available while the service is stopped.') : 'Choose a TOML configuration to start USAHP.';
  $('#config-path').textContent = snapshot.config_path ?? 'Not selected';
  $('#address').textContent = snapshot.address ?? '—';
  const captureLabels: Record<CaptureStatus['availability'], string> = {
    available: snapshot.capture.active ? 'Capturing configured inputs' : 'Released to the operating system',
    permission_required: 'Accessibility permission required',
    unavailable: snapshot.capture.message ? `Unavailable — ${snapshot.capture.message}` : 'Unavailable',
  };
  $('#capture').textContent = captureLabels[snapshot.capture.availability];
  $('#start').toggleAttribute('disabled', !snapshot.configured || snapshot.phase === 'running' || snapshot.phase === 'starting');
  $('#stop').toggleAttribute('disabled', snapshot.phase !== 'running');
  const permissionRequired = snapshot.capture.availability === 'permission_required';
  $('#permission-card').toggleAttribute('hidden', !permissionRequired);
  const errorCard = $('#error-card');
  errorCard.toggleAttribute('hidden', !snapshot.error || permissionRequired);
  $('#error').textContent = snapshot.error ?? '';

  $('#connection-count').textContent = String(snapshot.connections.length);
  $('#connections').className = snapshot.connections.length ? 'table-wrap' : 'empty';
  $('#connections').innerHTML = snapshot.connections.length ? `<table><thead><tr><th>Application</th><th>Request</th><th>Connection</th></tr></thead><tbody>${snapshot.connections.map((client) => `<tr><td><strong>${text(client.app_id ?? 'Anonymous listener')}</strong>${client.pid ? `<small>PID ${client.pid}</small>` : ''}</td><td>${text(outcome(client.outcome))}${client.requested_mode ? `<small>${text(client.requested_mode)}</small>` : ''}</td><td><code>#${client.client_id}</code><small>${text(client.peer ?? 'in-process')}</small></td></tr>`).join('')}</tbody></table>` : 'No clients connected';

  $('#switch-count').textContent = String(snapshot.switches.length);
  $('#switches').className = snapshot.switches.length ? 'switches' : 'switches empty';
  $('#switches').innerHTML = snapshot.switches.length ? snapshot.switches.map((item) => `<div class="switch ${item.state}"><span></span><strong>${text(item.switch_id)}</strong><small>${text(item.state)}${item.confidence === undefined ? '' : ` · ${item.confidence}%`}</small></div>`).join('') : 'No switches loaded';

  $('#session').className = snapshot.active_session ? 'session' : 'empty';
  $('#session').innerHTML = snapshot.active_session ? `<strong>${text(snapshot.active_session.app_id)}</strong><span>Exclusive foreground</span>${snapshot.active_session.pid ? `<small>PID ${snapshot.active_session.pid}</small>` : '<small>No PID supplied</small>'}` : 'No managed session';
}

async function refresh() {
  try { render(await invoke<Snapshot>('service_snapshot')); }
  catch (error) { $('#summary').textContent = `Unable to read service: ${String(error)}`; }
}

async function chooseConfig() {
  const selected = await open({ multiple: false, directory: false, filters: [{ name: 'USAHP configuration', extensions: ['toml'] }] });
  if (typeof selected === 'string') {
    try { await invoke('choose_config', { path: selected }); await refresh(); }
    catch (error) { await refresh(); $('#summary').textContent = String(error); }
  }
}

function requestAction(action: 'stop' | 'quit') {
  if (current?.active_session) {
    pendingAction = action;
    $('#confirm-copy').textContent = `${current.active_session.app_id} currently owns the managed session. ${action === 'quit' ? 'Quitting' : 'Stopping the service'} will revoke it and release all switches.`;
    ($('#confirm') as HTMLDialogElement).showModal();
  } else {
    void perform(action);
  }
}

async function perform(action: 'stop' | 'quit') {
  if (action === 'stop') { await invoke('stop_service'); await refresh(); }
  else { await invoke('quit_usahp'); }
}

$('#start').addEventListener('click', async () => { await invoke('start_service'); await refresh(); });
$('#stop').addEventListener('click', () => requestAction('stop'));
$('#config').addEventListener('click', () => void chooseConfig());
$('#grant-permission').addEventListener('click', async () => {
  try {
    const granted = await invoke<boolean>('grant_capture_permission');
    await refresh();
    if (!granted) $('#summary').textContent = 'Enable USAHP Control in System Settings, then select Grant Accessibility Access again.';
  } catch (error) {
    await refresh();
    $('#summary').textContent = String(error);
  }
});
$('#quit').addEventListener('click', () => requestAction('quit'));
$('#confirm').addEventListener('close', () => { const dialog = $('#confirm') as HTMLDialogElement; if (dialog.returnValue === 'confirm' && pendingAction) void perform(pendingAction); pendingAction = null; });
void listen<'stop' | 'quit'>('confirm-service-action', ({ payload }) => requestAction(payload));
void refresh();
setInterval(() => void refresh(), 750);
