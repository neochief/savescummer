import { afterEach, expect, test, vi } from 'vitest';
import { interfaceClickSound, playInterfaceSound, prepareDrawerSounds } from './interfaceSounds';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  localStorage.clear();
});

test('the app uses its selected sound assets, and disabled playback stays silent', () => {
  const play = vi.fn().mockResolvedValue(undefined);
  const Audio = vi.fn().mockImplementation(function (this: { currentTime: number; play: typeof play }) {
    this.currentTime = 0;
    this.play = play;
  });
  vi.stubGlobal('Audio', Audio);
  playInterfaceSound('button', false);
  expect(play).not.toHaveBeenCalled();
  playInterfaceSound('button');
  expect(Audio).toHaveBeenCalledWith('/sounds/button.wav');
  expect(play).toHaveBeenCalledOnce();
  playInterfaceSound('success');
  expect(Audio).toHaveBeenCalledWith('/sounds/success.wav');
});

test('routine buttons stay quiet while checkboxes sound automatically', () => {
  const play = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('Audio', vi.fn().mockImplementation(function (this: { currentTime: number; play: typeof play }) {
    this.currentTime = 0;
    this.play = play;
  }));
  const click = (element: HTMLElement) => {
    element.addEventListener('click', (event) => interfaceClickSound(event, true));
    element.click();
  };
  click(document.createElement('button'));
  const submit = document.createElement('button');
  submit.className = 'dialog-primary';
  submit.type = 'submit';
  click(submit);
  expect(play).not.toHaveBeenCalled();
  const checkbox = document.createElement('input');
  checkbox.type = 'checkbox';
  click(checkbox);
  expect(play).toHaveBeenCalledOnce();
});

test('drawer audio is prepared before the first Info click', () => {
  const play = vi.fn().mockResolvedValue(undefined);
  const Audio = vi.fn().mockImplementation(function (this: { currentTime: number; play: typeof play }) {
    this.currentTime = 0;
    this.play = play;
  });
  vi.stubGlobal('Audio', Audio);
  prepareDrawerSounds();
  expect(Audio).toHaveBeenCalledWith('/sounds/drawer-open.wav');
  expect(Audio).toHaveBeenCalledWith('/sounds/drawer-close.wav');
  expect(play).not.toHaveBeenCalled();
  playInterfaceSound('drawer-open');
  expect(Audio).toHaveBeenCalledTimes(2);
  expect(play).toHaveBeenCalledOnce();
});
