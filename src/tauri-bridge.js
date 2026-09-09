import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

let pendingMove = { x: 0, y: 0 };
let moveScheduled = false;

function moveBy(x, y) {
  pendingMove.x += Number(x) || 0;
  pendingMove.y += Number(y) || 0;
  if (moveScheduled) return;
  moveScheduled = true;
  requestAnimationFrame(() => {
    const movement = pendingMove;
    pendingMove = { x: 0, y: 0 };
    moveScheduled = false;
    invoke('move_by', movement).catch(() => {});
  });
}

// Electron can forward mouse movement while a window ignores clicks. Tauri's
// native ignore-cursor API cannot, which would make the pet impossible to
// click again after the pointer leaves it. Keep this small window interactive.
function ignoreMouse() {}

window.desktopPet = {
  getSettings: () => invoke('get_settings'),
  updateSettings: (patch) => invoke('update_settings', { patch }),
  getWindowContext: () => invoke('get_window_context'),
  getUserActivity: () => invoke('get_user_activity'),
  moveBy,
  savePosition: () => invoke('save_position'),
  ignoreMouse,
  openSettings: () => invoke('open_settings'),
  showPetMenu: () => invoke('show_pet_menu'),
  startSettingsDrag: () => invoke('start_settings_drag'),
  hide: () => invoke('hide_window'),
  closeSettings: () => invoke('close_settings'),
  onMode: () => Promise.resolve(() => {}),
  onSettingsChanged: (callback) => listen('settings:changed', (event) => callback(event.payload)),
  onReact: (callback) => listen('pet:react', (event) => callback(event.payload))
};
