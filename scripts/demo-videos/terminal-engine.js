// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Shared fake-terminal typing-animation engine for the demo-video pipeline.
// Each page sets `window.TERM_STEPS` (and optionally `window.TERM_PROMPT`)
// before this script runs. Steps: { type: 'cmd', text } or
// { type: 'out', text, cls } (cls: 'plain' | 'ok' | 'bad' | 'hl').
// Signals completion via `window.__demoDone = true`, matching the
// convention every seg-*.mjs Playwright recorder waits on.
(function () {
  const term = document.getElementById('term');
  const PROMPT = window.TERM_PROMPT || 'sus@lab-host:~$ ';
  const steps = window.TERM_STEPS || [];

  function scrollToEnd() { term.scrollTop = term.scrollHeight; }

  async function typeLine(prefix, prefixCls, text, cls, speed) {
    const line = document.createElement('div');
    line.className = 'line';
    const pre = document.createElement('span');
    pre.className = prefixCls;
    pre.textContent = prefix;
    line.appendChild(pre);
    const span = document.createElement('span');
    span.className = cls;
    line.appendChild(span);
    const cursor = document.createElement('span');
    cursor.className = 'cursor';
    line.appendChild(cursor);
    term.appendChild(line);
    scrollToEnd();
    for (let i = 0; i < text.length; i++) {
      span.textContent += text[i];
      if (text[i] === '\n') scrollToEnd();
      await new Promise((r) => setTimeout(r, speed));
    }
    cursor.remove();
    return line;
  }

  async function printBlock(text, cls, pause) {
    const pre = document.createElement('div');
    pre.className = 'line out ' + (cls || '');
    pre.textContent = text;
    term.appendChild(pre);
    scrollToEnd();
    await new Promise((r) => setTimeout(r, pause == null ? 250 : pause));
  }

  async function run() {
    await new Promise((r) => setTimeout(r, 800));
    for (const step of steps) {
      if (step.type === 'cmd') {
        await typeLine(PROMPT, 'prompt', step.text, 'cmd', step.speed || 26);
        await new Promise((r) => setTimeout(r, 350));
      } else if (step.type === 'pause') {
        await new Promise((r) => setTimeout(r, step.ms || 800));
      } else {
        await printBlock(step.text, step.cls, step.hold);
        await new Promise((r) => setTimeout(r, step.afterHold == null ? 1400 : step.afterHold));
      }
    }
    if (steps.length === 0 || steps[steps.length - 1].noPrompt !== true) {
      const finalLine = document.createElement('div');
      finalLine.className = 'line';
      const pre = document.createElement('span');
      pre.className = 'prompt';
      pre.textContent = PROMPT;
      finalLine.appendChild(pre);
      const cursor = document.createElement('span');
      cursor.className = 'cursor';
      finalLine.appendChild(cursor);
      term.appendChild(finalLine);
      scrollToEnd();
    }
    await new Promise((r) => setTimeout(r, 2500));
    document.title = 'DONE';
    window.__demoDone = true;
  }

  run();
})();
