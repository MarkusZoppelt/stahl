// The C functions ios/rust exports, for Swift.
#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef struct StahlHost StahlHost;

enum { STAHL_TICK_DREW = 1, STAHL_TICK_WANTS_KEYBOARD = 2 };

StahlHost *stahl_ios_new(void *metal_layer, const char *documents, float width, float height,
                         float scale, bool dark);
void stahl_ios_free(StahlHost *host);
uint32_t stahl_ios_tick(StahlHost *host);
void stahl_ios_resize(StahlHost *host, float width, float height, float scale);
// phase: 0 began, 1 moved, 2 ended, 3 cancelled.
void stahl_ios_touch(StahlHost *host, uint64_t id, uint32_t phase, float x, float y, double time);
void stahl_ios_insert_text(StahlHost *host, const char *text);
void stahl_ios_delete_backward(StahlHost *host);
