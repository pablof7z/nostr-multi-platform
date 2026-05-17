#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdint.h>

void *nmp_app_new(void);
void nmp_app_free(void *app);
typedef void (*NmpUpdateCallback)(void *context, const char *json);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
void nmp_app_start(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_configure(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);
void nmp_app_reset(void *app);
void nmp_app_open_author(void *app, const char *pubkey);
void nmp_app_open_thread(void *app, const char *event_id);
void nmp_app_open_firehose_tag(void *app, const char *tag);
void nmp_app_close_author(void *app, const char *pubkey);
void nmp_app_close_thread(void *app, const char *event_id);

#endif
