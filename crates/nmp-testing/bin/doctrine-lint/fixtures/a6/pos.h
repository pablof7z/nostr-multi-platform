// A6 positive fixture — C/Obj-C header declaring the banned C-ABI symbol.
//
// This simulates a native bridge header (e.g. NmpCore.h) where someone
// reintroduced `nmp_app_register_snapshot_projection` after escape hatch #2
// was eliminated. The lint must flag this line even though it is a `.h` file.

#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdint.h>
#include <stdbool.h>

typedef void NmpApp;
typedef void (*NmpProjectionFn)(void);

// A6 violation: the generic JSON projection C-ABI symbol is banned.
void nmp_app_register_snapshot_projection(NmpApp *app, const char *key, NmpProjectionFn fn);

// This typed variant is the survivor and must NOT be flagged.
void nmp_app_register_typed_snapshot_projection(NmpApp *app, const char *key, NmpProjectionFn fn);

#endif // NMP_CORE_H
