import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { Window } from 'happy-dom';

import { useTerminalStore, type TerminalChunk } from '@/stores/useTerminalStore';

type TerminalEvent =
  | { type: 'write'; data: string }
  | { type: 'reset' }
  | { type: 'resize'; cols: number; rows: number };
const terminalEvents: TerminalEvent[] = [];

class GhosttyTerminalDouble {
  public options: { cursorBlink: boolean };
  public cols = 80;
  public rows = 24;

  constructor(options: { cursorBlink?: boolean; cols?: number; rows?: number }) {
    this.options = { cursorBlink: options.cursorBlink ?? false };
    this.cols = options.cols ?? 80;
    this.rows = options.rows ?? 24;
  }

  loadAddon() {}
  open() {}
  onData() {
    return { dispose() {} };
  }
  write(data: string, callback?: () => void) {
    terminalEvents.push({ type: 'write', data });
    callback?.();
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    terminalEvents.push({ type: 'resize', cols, rows });
  }
  reset() {
    terminalEvents.push({ type: 'reset' });
  }
  focus() {}
  dispose() {}
}

class FitAddonDouble {
  fit() {}
}

mock.module('ghostty-web', () => ({
  Ghostty: { load: async () => ({}) },
  Terminal: GhosttyTerminalDouble,
  FitAddon: FitAddonDouble,
}));

const { TerminalViewport } = await import('./TerminalViewport');

const theme = {
  background: '#000000',
  foreground: '#ffffff',
  cursor: '#ffffff',
  cursorAccent: '#000000',
  selectionBackground: '#334155',
  selectionForeground: '#ffffff',
  black: '#111111',
  red: '#ff0000',
  green: '#00ff00',
  yellow: '#ffff00',
  blue: '#0000ff',
  magenta: '#ff00ff',
  cyan: '#00ffff',
  white: '#ffffff',
  brightBlack: '#666666',
  brightRed: '#ff0000',
  brightGreen: '#00ff00',
  brightYellow: '#ffff00',
  brightBlue: '#0000ff',
  brightMagenta: '#ff00ff',
  brightCyan: '#00ffff',
  brightWhite: '#ffffff',
} as const;

const flushGhosttyLoad = async () => {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};

const TERMINAL_BUFFER_CAP = 512 * 1024;

const replayWriteEvents = (expectedPayloads: string[]) => terminalEvents.filter(
  (event): event is { type: 'write'; data: string } => event.type === 'write' && expectedPayloads.includes(event.data),
);

const buildReplacedBufferChunks = (content: string): TerminalChunk[] => {
  const directory = '/fixture';
  useTerminalStore.getState().clearAll();
  useTerminalStore.getState().ensureDirectory(directory);
  const tabId = useTerminalStore.getState().getDirectoryState(directory)?.tabs[0]?.id;
  if (!tabId) throw new Error('fixture tab missing');
  useTerminalStore.getState().replaceBuffer(directory, tabId, content, 1);
  return [...useTerminalStore.getState().getBuffer(directory, tabId).chunks];
};

const renderViewport = (root: Root, chunks: TerminalChunk[]) => act(async () => {
  root.render(
    <TerminalViewport
      sessionKey="session-1"
      chunks={chunks}
      onInput={() => undefined}
      onResize={() => undefined}
      theme={theme}
      monoFont="geist-mono"
      fontFamily="Geist Mono"
      fontSize={14}
    />,
  );
});

describe('TerminalViewport chunk replay integration', () => {
  let windowInstance: Window;
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    terminalEvents.length = 0;
    useTerminalStore.getState().clearAll();
    windowInstance = new Window({ url: 'http://localhost/' });
    Object.assign(globalThis, {
      window: windowInstance,
      document: windowInstance.document,
      navigator: windowInstance.navigator,
      HTMLElement: windowInstance.HTMLElement,
      Element: windowInstance.Element,
      Node: windowInstance.Node,
      Event: windowInstance.Event,
      InputEvent: windowInstance.InputEvent,
      KeyboardEvent: windowInstance.KeyboardEvent,
      MouseEvent: windowInstance.MouseEvent,
      FocusEvent: windowInstance.FocusEvent,
      ResizeObserver: class {
        observe() {}
        disconnect() {}
      },
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      },
      cancelAnimationFrame: () => undefined,
      IS_REACT_ACT_ENVIRONMENT: true,
    });
    Object.defineProperty(windowInstance.document, 'hasFocus', {
      configurable: true,
      value: () => true,
    });
    Object.defineProperty(windowInstance.HTMLElement.prototype, 'getBoundingClientRect', {
      configurable: true,
      value() {
        return { x: 0, y: 0, top: 0, left: 0, right: 800, bottom: 600, width: 800, height: 600 };
      },
    });

    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useTerminalStore.getState().clearAll();
  });

  test('would fail if adopted-buffer remount replay split history writes or exceeded the capped buffer payload', async () => {
    const replayChunks: TerminalChunk[] = [
      { id: 1, data: 'live-one\n', replayData: 'replay-one\n', byteLength: 9 },
      { id: 2, data: 'live-two\n', replayData: 'replay-two\n', byteLength: 9 },
      { id: 3, data: 'live-three\n', byteLength: 11 },
    ];
    const replayPayload = 'replay-one\nreplay-two\nlive-three\n';

    await renderViewport(root, replayChunks);
    await flushGhosttyLoad();

    expect(terminalEvents.filter((event) => event.type === 'reset')).toHaveLength(0);
    expect(replayWriteEvents([replayPayload])).toEqual([{ type: 'write', data: replayPayload }]);

    await act(async () => root.unmount());
    host.remove();
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    terminalEvents.length = 0;

    const oversizedReplayChunks = buildReplacedBufferChunks(`${'🙂'.repeat(180_000)}tail`);
    const oversizedPayload = oversizedReplayChunks.map((chunk) => chunk.data).join('');

    await renderViewport(root, oversizedReplayChunks);
    await flushGhosttyLoad();

    expect(replayWriteEvents([oversizedPayload])).toEqual([{ type: 'write', data: oversizedPayload }]);
    expect(new TextEncoder().encode(oversizedPayload).byteLength).toBeLessThanOrEqual(TERMINAL_BUFFER_CAP);
  });

  test('would fail if authoritative replacement replay reset twice or re-streamed replacement history chunk-by-chunk', async () => {
    const initialChunks: TerminalChunk[] = [
      { id: 1, data: 'initial-live\n', replayData: 'initial-replay\n', byteLength: 13 },
    ];
    const appendedChunks: TerminalChunk[] = [
      ...initialChunks,
      { id: 2, data: 'append-live\n', replayData: 'append-replay\n', byteLength: 12 },
    ];
    const replacementChunks: TerminalChunk[] = [
      { id: 3, data: 'history-live-1\n', replayData: 'history-replay-1\n', byteLength: 15 },
      { id: 4, data: 'history-live-2\n', replayData: 'history-replay-2\n', byteLength: 15 },
    ];
    const replacementReplayPayload = 'history-replay-1\nhistory-replay-2\n';

    await renderViewport(root, initialChunks);
    await flushGhosttyLoad();
    terminalEvents.length = 0;

    await renderViewport(root, appendedChunks);
    expect(terminalEvents).toEqual([{ type: 'write', data: 'append-live\n' }]);

    terminalEvents.length = 0;
    await renderViewport(root, replacementChunks);
    expect(terminalEvents.filter((event) => event.type === 'reset')).toHaveLength(1);
    expect(replayWriteEvents([replacementReplayPayload])).toEqual([{ type: 'write', data: replacementReplayPayload }]);
    expect(terminalEvents.some((event) => event.type === 'write' && event.data === 'history-replay-1\n')).toBe(false);
    expect(terminalEvents.some((event) => event.type === 'write' && event.data === 'history-replay-2\n')).toBe(false);
    expect(terminalEvents.some((event) => event.type === 'write' && event.data === 'history-live-1\n')).toBe(false);
    expect(terminalEvents.some((event) => event.type === 'write' && event.data === 'history-live-2\n')).toBe(false);
  });

  test('would fail if a live append after replacement replay duplicated history or lost the new chunk ordering', async () => {
    const initialChunks: TerminalChunk[] = [
      { id: 1, data: 'initial-live\n', replayData: 'initial-replay\n', byteLength: 13 },
    ];
    const replacementChunks: TerminalChunk[] = [
      { id: 3, data: 'history-live-1\n', replayData: 'history-replay-1\n', byteLength: 15 },
      { id: 4, data: 'history-live-2\n', replayData: 'history-replay-2\n', byteLength: 15 },
    ];
    const resumedChunks: TerminalChunk[] = [
      ...replacementChunks,
      { id: 5, data: 'tail-live\n', replayData: 'tail-replay\n', byteLength: 10 },
    ];
    const replacementReplayPayload = 'history-replay-1\nhistory-replay-2\n';

    await renderViewport(root, initialChunks);
    await flushGhosttyLoad();

    terminalEvents.length = 0;
    await renderViewport(root, replacementChunks);
    await renderViewport(root, resumedChunks);

    expect(terminalEvents.filter((event) => event.type === 'reset')).toHaveLength(1);
    expect(replayWriteEvents([replacementReplayPayload, 'tail-live\n'])).toEqual([
      { type: 'write', data: replacementReplayPayload },
      { type: 'write', data: 'tail-live\n' },
    ]);
    expect(terminalEvents.filter((event) => event.type === 'write' && event.data === replacementReplayPayload)).toHaveLength(1);
    expect(terminalEvents.filter((event) => event.type === 'write' && event.data === 'tail-live\n')).toHaveLength(1);
  });

  test('would fail if snapshot history drawn for another PTY size were replayed at the fitted size', async () => {
    // A zsh prompt drawn for a 94-column PTY: the `%` end-of-line mark plus
    // padding fills exactly one 94-column row. Written into an 80-column
    // emulator it wraps and the mark survives as a stray fragment.
    const history = `[7m%[0m${' '.repeat(93)}\r \r[J~ ❯ `;
    const chunks: TerminalChunk[] = [
      { id: 1, data: history, byteLength: history.length, size: { cols: 94, rows: 56 } },
      { id: 2, data: 'live\n', byteLength: 5 },
    ];

    await renderViewport(root, chunks);
    await flushGhosttyLoad();

    // Default-background resets inside the history are rewritten before the
    // write, so identify the history write by the prompt it carries.
    const relevant = terminalEvents
      .filter((event) => event.type === 'resize' || (event.type === 'write' && (event.data.includes('~ ❯') || event.data === 'live\n')))
      .map((event) => (event.type === 'write' && event.data.includes('~ ❯') ? { type: 'write', data: 'history' } : event));
    expect(relevant).toEqual([
      { type: 'resize', cols: 94, rows: 56 },
      { type: 'write', data: 'history' },
      { type: 'resize', cols: 80, rows: 24 },
      { type: 'write', data: 'live\n' },
    ]);

    terminalEvents.length = 0;
    await renderViewport(root, [...chunks, { id: 3, data: 'more\n', byteLength: 5 }]);
    expect(terminalEvents).toEqual([{ type: 'write', data: 'more\n' }]);
  });

  test('would fail if a snapshot drawn at the fitted size still bounced the emulator through a resize', async () => {
    const chunks: TerminalChunk[] = [
      { id: 1, data: 'prompt ❯ ', byteLength: 11, size: { cols: 80, rows: 24 } },
    ];

    await renderViewport(root, chunks);
    await flushGhosttyLoad();

    expect(terminalEvents.filter((event) => event.type === 'resize')).toHaveLength(0);
    expect(replayWriteEvents(['prompt ❯ '])).toEqual([{ type: 'write', data: 'prompt ❯ ' }]);
  });
});
