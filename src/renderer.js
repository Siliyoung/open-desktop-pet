import './tauri-bridge.js';
import pigIdleUrl from './assets/pig-idle.png';
import pigThinkingUrl from './assets/pig-thinking.png';
import pigHappyUrl from './assets/pig-happy.png';
import pigSadUrl from './assets/pig-sad.png';
import pigWaveUrl from './assets/pig-wave.png';
import pigSleepUrl from './assets/pig-sleep.png';
import pigPettedUrl from './assets/pig-petted.png';

const api = window.desktopPet;
const petView = document.querySelector('#pet-view');
const settingsView = document.querySelector('#settings-view');
const pet = document.querySelector('#pet');
const petImage = document.querySelector('#pet-image');
const speech = document.querySelector('#speech');
const form = document.querySelector('#settings-form');
const scaleInput = document.querySelector('#pet-scale');
const scaleValue = document.querySelector('#scale-value');
const saveStatus = document.querySelector('#save-status');
const initialMode = new URLSearchParams(window.location.search).get('mode') === 'settings' ? 'settings' : 'pet';

let settings;
let stateTimer;
let speechTimer;
let sleepTimer;
let wanderTimer;
let wanderStepTimer;
let activityTimer;
let dragging = false;
let dragStart;
let dragPoint;
let audioContext;
let longPressTimer;
let longPressHandled = false;
let currentMode = initialMode;
let currentState = 'idle';
let userWorking = false;
let interactionUntil = 0;

const phrases = ['记得喝水呀', '今天也辛苦啦', '伸个懒腰吧', '我会安静陪着你', '要不要休息五分钟？'];
const tapMessages = ['哼哼？', '今天也来啦～', '记录点什么吧？', '今天也要加油！', '我一直都在～', '戳到我啦！'];
const pettedMessages = ['嘿嘿～', '好舒服。', '再摸一下嘛～', '哼哼～', '今天也陪着你。'];
const petImages = {
  idle: pigIdleUrl, thinking: pigThinkingUrl, happy: pigHappyUrl,
  sad: pigSadUrl, wave: pigWaveUrl, sleep: pigSleepUrl,
  petted: pigPettedUrl, walk: pigIdleUrl
};

function setState(state, duration = 0) {
  clearTimeout(stateTimer);
  const safeState = petImages[state] ? state : 'idle';
  currentState = safeState;
  if (duration) interactionUntil = Date.now() + duration;
  petImage.src = petImages[safeState];
  pet.className = `pet state-${safeState} interactive`;
  if (safeState === 'petted') pet.classList.add('show-heart');
  if (duration) stateTimer = setTimeout(() => setState('idle'), duration);
  resetSleepTimer();
}

function say(message, duration = 3200) {
  clearTimeout(speechTimer);
  speech.textContent = message;
  speech.classList.add('visible');
  speechTimer = setTimeout(() => speech.classList.remove('visible'), duration);
}

function beep() {
  if (!settings?.sound) return;
  try {
    audioContext ||= new AudioContext();
    const oscillator = audioContext.createOscillator();
    const gain = audioContext.createGain();
    oscillator.type = 'sine';
    oscillator.frequency.setValueAtTime(560, audioContext.currentTime);
    oscillator.frequency.exponentialRampToValueAtTime(760, audioContext.currentTime + .09);
    gain.gain.setValueAtTime(.045, audioContext.currentTime);
    gain.gain.exponentialRampToValueAtTime(.001, audioContext.currentTime + .13);
    oscillator.connect(gain).connect(audioContext.destination);
    oscillator.start();
    oscillator.stop(audioContext.currentTime + .14);
  } catch { /* Audio is optional. */ }
}

function resetSleepTimer() {
  clearTimeout(sleepTimer);
  sleepTimer = setTimeout(() => { if (!userWorking) setState('sleep'); }, 90_000);
}

function scheduleWander(delay = 18_000 + Math.random() * 22_000) {
  clearTimeout(wanderTimer);
  clearInterval(wanderStepTimer);
  if (!settings?.wandering || currentMode !== 'pet' || userWorking) return;
  wanderTimer = setTimeout(async () => {
    if (document.hidden || dragging || userWorking || !settings.wandering) return scheduleWander();
    try {
      const { bounds, target } = await api.getWindowContext();
      let remainingX = target.x - bounds.x;
      let remainingY = target.y - bounds.y;
      let remainingDistance = Math.hypot(remainingX, remainingY);
      if (remainingDistance < 1) return scheduleWander(1200);

      setState('walk');
      if (remainingX < 0) pet.classList.add('facing-left');
      wanderStepTimer = setInterval(() => {
        if (!settings.wandering || dragging || userWorking || currentMode !== 'pet') {
          clearInterval(wanderStepTimer);
          setState('idle');
          return scheduleWander();
        }
        const distance = Math.hypot(remainingX, remainingY);
        if (distance <= 3) {
          api.moveBy(remainingX, remainingY);
          api.savePosition();
          clearInterval(wanderStepTimer);
          setState('idle');
          if (!target.joining && Math.random() > .55) say(phrases[Math.floor(Math.random() * phrases.length)]);
          return scheduleWander(target.joining ? 900 : 2200 + Math.random() * 2200);
        }
        const step = Math.min(2.5, distance);
        const moveX = remainingX / distance * step;
        const moveY = remainingY / distance * step;
        api.moveBy(moveX, moveY);
        remainingX -= moveX;
        remainingY -= moveY;
        remainingDistance = distance - step;
      }, 35);
    } catch {
      setState('idle');
      scheduleWander();
    }
  }, delay);
}

async function pollUserActivity() {
  if (currentMode !== 'pet') return;
  try {
    const idleMs = await api.getUserIdleMs();
    const activeNow = idleMs < 2200;
    if (activeNow) {
      userWorking = true;
      clearTimeout(wanderTimer);
      clearInterval(wanderStepTimer);
      if (Date.now() < interactionUntil || dragging) return;
      speech.classList.remove('visible');
      if (currentState !== 'thinking') setState('thinking');
    } else if (userWorking && idleMs > 3800) {
      userWorking = false;
      if (currentState === 'thinking') setState('idle');
      scheduleWander(8000);
    }
  } catch { /* Activity detection is optional on unsupported systems. */ }
}

function startActivityMonitoring() {
  clearInterval(activityTimer);
  pollUserActivity();
  activityTimer = setInterval(pollUserActivity, 800);
}

function applySettings(next) {
  settings = next;
  document.documentElement.style.setProperty('--pet-scale', settings.scale);
  document.documentElement.style.setProperty('--bubble-bottom', `${Math.min(210, 17 + 165 * settings.scale)}px`);
  document.documentElement.style.setProperty('--hide-bottom', `${Math.max(8, -13 + 165 * settings.scale)}px`);
  document.documentElement.style.setProperty('--hide-right', `${Math.max(8, 95 - 82.5 * settings.scale)}px`);
  pet.setAttribute('aria-label', `摸摸${settings.petName}`);
  if (currentMode === 'pet') scheduleWander();
}

function populateForm() {
  form.elements.petName.value = settings.petName;
  form.elements.scale.value = settings.scale;
  form.elements.alwaysOnTop.checked = settings.alwaysOnTop;
  form.elements.wandering.checked = settings.wandering;
  form.elements.sound.checked = settings.sound;
  form.elements.autoStart.checked = settings.autoStart;
  scaleValue.value = `${Math.round(settings.scale * 100)}%`;
}

function switchMode(mode) {
  const isSettings = mode === 'settings';
  currentMode = mode;
  settingsView.hidden = !isSettings;
  petView.hidden = isSettings;
  api.ignoreMouse(!isSettings);
  if (isSettings) {
    clearInterval(activityTimer);
    clearTimeout(wanderTimer);
    clearInterval(wanderStepTimer);
    clearTimeout(sleepTimer);
    populateForm();
  } else {
    startActivityMonitoring();
    scheduleWander();
    resetSleepTimer();
  }
}

pet.addEventListener('pointerdown', (event) => {
  if (event.button !== 0) return;
  dragging = true;
  clearTimeout(wanderTimer);
  clearInterval(wanderStepTimer);
  dragStart = { x: event.screenX, y: event.screenY };
  dragPoint = { ...dragStart };
  longPressHandled = false;
  clearTimeout(longPressTimer);
  longPressTimer = setTimeout(() => {
    if (!dragging) return;
    longPressHandled = true;
    setState('petted', 1500);
    say(pettedMessages[Math.floor(Math.random() * pettedMessages.length)], 2000);
    beep();
  }, 650);
  pet.setPointerCapture(event.pointerId);
  api.ignoreMouse(false);
});

pet.addEventListener('pointermove', (event) => {
  if (!dragging) return;
  const totalTravel = Math.hypot(event.screenX - dragStart.x, event.screenY - dragStart.y);
  if (totalTravel > 8) clearTimeout(longPressTimer);
  const delta = { x: event.screenX - dragPoint.x, y: event.screenY - dragPoint.y };
  dragPoint = { x: event.screenX, y: event.screenY };
  api.moveBy(delta.x, delta.y);
});

pet.addEventListener('pointerup', (event) => {
  if (!dragging) return;
  clearTimeout(longPressTimer);
  dragging = false;
  pet.releasePointerCapture(event.pointerId);
  api.ignoreMouse(true);
  api.savePosition();
  const travel = Math.hypot(event.screenX - dragStart.x, event.screenY - dragStart.y);
  if (!longPressHandled && travel < 6) {
    setState('wave', 1000);
    say(tapMessages[Math.floor(Math.random() * tapMessages.length)], 2000);
    beep();
  }
  scheduleWander();
});

pet.addEventListener('pointercancel', () => {
  clearTimeout(longPressTimer);
  dragging = false;
  longPressHandled = false;
  api.ignoreMouse(true);
});

async function openSettings() {
  try {
    await api.showMenu();
  } catch {
    say('设置窗口暂时打不开，请从托盘重试。');
  }
}

pet.addEventListener('contextmenu', (event) => { event.preventDefault(); openSettings(); });
document.querySelector('#settings-pet').addEventListener('click', openSettings);
document.querySelector('#hide-pet').addEventListener('click', () => api.hide());
document.querySelector('#close-settings').addEventListener('click', () => api.closeSettings());
scaleInput.addEventListener('input', () => { scaleValue.value = `${Math.round(Number(scaleInput.value) * 100)}%`; });

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  const next = await api.updateSettings({
    petName: form.elements.petName.value,
    scale: Number(form.elements.scale.value),
    alwaysOnTop: form.elements.alwaysOnTop.checked,
    wandering: form.elements.wandering.checked,
    sound: form.elements.sound.checked,
    autoStart: form.elements.autoStart.checked
  });
  applySettings(next);
  saveStatus.textContent = '已保存';
  setTimeout(() => {
    saveStatus.textContent = '';
    api.closeSettings();
  }, 350);
});

document.addEventListener('mousemove', (event) => {
  if (settingsView.hidden && !dragging) api.ignoreMouse(!event.target.closest('.interactive'));
});
document.addEventListener('mouseleave', () => { if (settingsView.hidden && !dragging) api.ignoreMouse(true); });

api.onMode(switchMode);
api.onSettingsChanged(applySettings);
api.onReact(({ state, message }) => { setState(state, 1400); say(message); beep(); });

api.getSettings().then((value) => {
  applySettings(value);
  switchMode(initialMode);
  if (initialMode === 'pet') say(`你好，我是${settings.petName}`);
});
