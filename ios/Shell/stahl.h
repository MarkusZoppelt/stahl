// The C functions ios/rust exports, for Swift.
#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef struct StahlHost StahlHost;

// What stahl_ios_tick says, as bits: see the TICK_ constants in ios/rust/src/lib.rs.
enum {
    STAHL_TICK_DREW = 1,
    STAHL_TICK_WANTS_KEYBOARD = 2,
    // The app asks for a file or a folder: present a document picker, then call stahl_ios_picked.
    STAHL_TICK_PICK_FILE = 4,
    STAHL_TICK_PICK_FOLDER = 8,
    STAHL_TICK_SECURE = 16,
    STAHL_TICK_NUMBER = 32,
    // The app copied something: stahl_ios_take_copied has it.
    STAHL_TICK_COPIED = 64,
    STAHL_TICK_HAPTIC_SELECT = 128,
    STAHL_TICK_HAPTIC_SUCCESS = 256,
    STAHL_TICK_HAPTIC_WARNING = 512,
    STAHL_TICK_HAPTIC_TURN = 1024,
    STAHL_TICK_HAPTIC_TICK = 2048,
    // The lock setting changed: stahl_ios_lock_enabled says what to remember.
    STAHL_TICK_LOCK_CHANGED = 4096,
};

StahlHost *stahl_ios_new(void *metal_layer, const char *documents, float width, float height,
                         float scale, bool dark);
void stahl_ios_free(StahlHost *host);
uint32_t stahl_ios_tick(StahlHost *host);
// The picker's answer: a path the shell can access, or NULL when cancelled.
void stahl_ios_picked(StahlHost *host, const char *path);
void stahl_ios_set_compact(StahlHost *host, bool compact);
// The app's background, 0xRRGGBB in sRGB.
uint32_t stahl_ios_background(StahlHost *host);
void stahl_ios_resize(StahlHost *host, float width, float height, float scale);
// phase: 0 began, 1 moved, 2 ended, 3 cancelled.
void stahl_ios_touch(StahlHost *host, uint64_t id, uint32_t phase, float x, float y, double time);
// A hardware key: 0 for a chord with Command and `character`, or one of these.
enum {
    STAHL_KEY_CHORD = 0, STAHL_KEY_ESCAPE, STAHL_KEY_TAB, STAHL_KEY_ENTER, STAHL_KEY_BACKSPACE,
    STAHL_KEY_DELETE, STAHL_KEY_LEFT, STAHL_KEY_RIGHT, STAHL_KEY_UP, STAHL_KEY_DOWN,
    STAHL_KEY_HOME, STAHL_KEY_END, STAHL_KEY_PAGE_UP, STAHL_KEY_PAGE_DOWN,
};
// mods: 1 Shift, 2 Command, 4 Option, 8 Control.
void stahl_ios_key(StahlHost *host, uint32_t key, uint32_t character, uint32_t mods);
void stahl_ios_hover(StahlHost *host, float x, float y, bool inside);
// Two-finger scrolling, the way the fingers moved. phase: 0 began, 1 moved, 2 ended.
void stahl_ios_scroll(StahlHost *host, float x, float y, float dx, float dy, uint32_t phase);
void stahl_ios_insert_text(StahlHost *host, const char *text);
void stahl_ios_delete_backward(StahlHost *host);

// What the app last copied, to free with stahl_ios_free_string; NULL if nothing.
char *stahl_ios_take_copied(StahlHost *host);
void stahl_ios_free_string(char *text);
void stahl_ios_paste(StahlHost *host, const char *text);
// The app goes to the background: unsaved edits are saved. Returns whether a save started.
bool stahl_ios_backgrounded(StahlHost *host);
void stahl_ios_foregrounded(StahlHost *host);
void stahl_ios_shake(StahlHost *host);
void stahl_ios_set_reduced_motion(StahlHost *host, bool reduced);
// A software keyboard covers the bottom of the app, or no longer does.
void stahl_ios_set_keyboard(StahlHost *host, bool up);
// The back gesture, a swipe in from the left edge. Returns whether it went back.
bool stahl_ios_back(StahlHost *host);
// Whether the device can lock the app, and whether to: name is "Face ID", "Touch ID" or "Passcode".
void stahl_ios_set_lock(StahlHost *host, bool available, bool enabled, const char *name);
bool stahl_ios_lock_enabled(StahlHost *host);
