/* whoate C ABI. Every returned string is UTF-8 JSON owned by the library and
 * must be released with whoate_string_free. Errors are JSON, not NULL:
 *   {"schema":"whoate/v1","error":{"code":N,"message":"..."}}
 *   code 1 = internal or sampling failure, 2 = invalid argument, 3 = not found.
 * One engine must be used from one thread at a time. */
#ifndef WHOATE_H
#define WHOATE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WhoateEngine WhoateEngine;

/* NULL when the native sampler, configuration or knowledge base fails. */
WhoateEngine *whoate_engine_new(void);

/* whoate/v1 report. Each owner's history holds at most 60 evenly spaced
 * points ending at the newest one. */
char *whoate_engine_tick(WhoateEngine *engine);

/* Report holding only the owner(s) containing pid, with full history.
 * Ticks once first if the engine has not ticked yet. */
char *whoate_why(WhoateEngine *engine, int32_t pid);

/* Returns 0, 3 (a pid is missing), 4 (a pid was refused; wins over 3) or
 * 2 (pids NULL or n <= 0). force = 0 sends SIGTERM to identities from the
 * latest live tick and waits 5 seconds; force = 1 sends SIGKILL only to pids
 * still alive after that request, reusing its identities. When err is not
 * NULL, *err receives {"schema","executed","results":[{pid,status,reason}],
 * "missing_targets","memory"} with status terminated / still_alive /
 * refused / missing; free it with whoate_string_free. */
int32_t whoate_kill(const int32_t *pids, int32_t n, int32_t force, char **err);

/* {"schema":"whoate/v1","insights":[...]}, same entries as
 * `whoate insights --list`. */
char *whoate_insights_json(void);

/* NULL is a no-op for both. */
void whoate_string_free(char *s);
void whoate_engine_free(WhoateEngine *engine);

#ifdef WHOATE_DEBUG
/* Debug builds only. A file with "owners" replays as a report; otherwise it
 * is a Snapshot run through the full pipeline. NULL on any failure. */
WhoateEngine *whoate_engine_new_from_fixture(const char *path);
#endif

#ifdef __cplusplus
}
#endif

#endif
