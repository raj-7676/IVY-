import {StrictMode} from 'react';
import {createRoot} from 'react-dom/client';
import CapsuleWindow from './CapsuleWindow.tsx';
import './index.css';

// Dedicated entry point for the capsule window — never shares index.html
// with the main app window. A shared entry meant this page's real
// transparency depended on JS detecting the window label and overriding
// whatever the main app's own markup/CSS painted first; a wrong or late
// detection left the capsule showing the main window's own background.
// This file has nothing to inherit or override: html/body/#root start
// transparent in capsule.html's own inline <style>, nothing else in this
// bundle ever paints them.
function showFatalError(err: unknown) {
  const root = document.getElementById('root');
  if (!root) return;
  const message = err instanceof Error ? `${err.name}: ${err.message}\n${err.stack ?? ''}` : String(err);
  // DOM-safe: no innerHTML, no XSS surface — textContent never interprets HTML
  root.textContent = '';
  const pre = document.createElement('pre');
  pre.style.cssText = 'color:#ff6b6b;background:#171717;padding:16px;font:12px monospace;white-space:pre-wrap;height:100vh;margin:0;overflow:auto;';
  pre.textContent = `Ivy capsule failed to start:\n\n${message}`;
  root.appendChild(pre);
}

window.addEventListener('error', (e) => showFatalError(e.error ?? e.message));
window.addEventListener('unhandledrejection', (e) => showFatalError(e.reason));

try {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <CapsuleWindow />
    </StrictMode>,
  );
} catch (err) {
  showFatalError(err);
}
