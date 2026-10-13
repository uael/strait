// The strait engine (crates/strait-mac/src/ffi.rs).
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef void (*strait_status_fn)(const char *json, void *context);
typedef void (*strait_clipboard_fn)(uint32_t kind, const uint8_t *bytes, size_t len, void *context);

enum { STRAIT_CLIP_TEXT = 0, STRAIT_CLIP_PNG = 1 };

// Starts the engine with its files in dir. Callbacks arrive on engine threads.
bool strait_start(const char *dir, strait_status_fn on_status, strait_clipboard_fn on_clipboard, void *context);
// Runs one command given as a JSON object, such as {"preset":"split"}.
void strait_command(const char *json);
// Hands the Mac's clipboard to Windows.
void strait_clipboard(uint32_t kind, const uint8_t *bytes, size_t len);
