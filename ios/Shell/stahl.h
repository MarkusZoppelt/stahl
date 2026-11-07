// The C functions ios/rust exports, for Swift.
#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef struct StahlHost StahlHost;

enum {
    STAHL_TICK_DREW = 1,
    STAHL_TICK_WANTS_KEYBOARD = 2,
    // The app asks for a file or a folder: present a document picker, then call stahl_ios_picked.
    STAHL_TICK_PICK_FILE = 4,
    STAHL_TICK_PICK_FOLDER = 8,
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
