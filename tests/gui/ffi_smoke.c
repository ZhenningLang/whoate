/* C ABI smoke test for crates/whoate-ffi/include/whoate.h (QA3).
 * Compiled with -Wall -Werror against both the debug and release static
 * libraries. Usage:
 *   ffi_smoke [--tick-out FILE] [--why-out FILE] [--insights-out FILE]
 *   ffi_smoke --bench N      CPU time per tick, no sleeping between ticks
 *   ffi_smoke --fixture PATH debug library only: replay engine must open
 */
#include "whoate.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <unistd.h>

static int failures = 0;
#define CHECK(cond, label)                                   \
    do {                                                     \
        if (!(cond)) {                                       \
            fprintf(stderr, "FAIL %s\n", label);             \
            failures++;                                      \
        }                                                    \
    } while (0)

static void save(const char *path, const char *text) {
    if (!path) return;
    FILE *f = fopen(path, "w");
    if (!f) { fprintf(stderr, "FAIL open %s\n", path); failures++; return; }
    fputs(text, f);
    fputc('\n', f);
    fclose(f);
}

/* First pid in the first "pids":[...] array of a report. */
static int first_pid(const char *json) {
    const char *p = strstr(json, "\"pids\":[");
    return p ? atoi(p + 8) : -1;
}

static double cpu_seconds(void) {
    struct rusage r;
    getrusage(RUSAGE_SELF, &r);
    return r.ru_utime.tv_sec + r.ru_stime.tv_sec + (r.ru_utime.tv_usec + r.ru_stime.tv_usec) / 1e6;
}

int main(int argc, char **argv) {
    const char *tick_out = NULL, *why_out = NULL, *insights_out = NULL, *fixture = NULL;
    int bench = 0;
    for (int i = 1; i + 1 < argc; i += 2) {
        if (!strcmp(argv[i], "--tick-out")) tick_out = argv[i + 1];
        else if (!strcmp(argv[i], "--why-out")) why_out = argv[i + 1];
        else if (!strcmp(argv[i], "--insights-out")) insights_out = argv[i + 1];
        else if (!strcmp(argv[i], "--bench")) bench = atoi(argv[i + 1]);
        else if (!strcmp(argv[i], "--fixture")) fixture = argv[i + 1];
    }

    /* NULL handling never crashes and always returns JSON or a code. */
    whoate_string_free(NULL);
    whoate_engine_free(NULL);
    char *null_tick = whoate_engine_tick(NULL);
    CHECK(null_tick && strstr(null_tick, "\"code\":2"), "null_engine_tick_error_json");
    whoate_string_free(null_tick);
    char *err = NULL;
    CHECK(whoate_kill(NULL, 1, 0, &err) == 2, "kill_null_pids_code_2");
    CHECK(err && strstr(err, "\"code\":2"), "kill_null_pids_err_json");
    whoate_string_free(err);
    CHECK(whoate_kill(NULL, 0, 0, NULL) == 2, "kill_null_err_pointer_safe");

    char *insights = whoate_insights_json();
    CHECK(insights && strstr(insights, "\"insights\":[{"), "insights_entries");
    if (insights) save(insights_out, insights);
    whoate_string_free(insights);

    WhoateEngine *engine = NULL;
#ifdef WHOATE_DEBUG
    if (fixture) {
        engine = whoate_engine_new_from_fixture(fixture);
        CHECK(engine != NULL, "fixture_engine_opens");
        CHECK(whoate_engine_new_from_fixture("/nonexistent/whoate.json") == NULL, "fixture_bad_path_null");
    }
#else
    CHECK(fixture == NULL, "fixture_requires_debug_library");
#endif
    if (!engine) engine = whoate_engine_new();
    CHECK(engine != NULL, "engine_new");
    if (!engine) return 1;

    char *tick = whoate_engine_tick(engine);
    CHECK(tick && !strncmp(tick, "{", 1) && strstr(tick, "\"schema\":\"whoate/v1\"") && strstr(tick, "\"owners\":["),
          "tick_report_json");
    int pid = tick ? first_pid(tick) : -1;
    CHECK(pid >= 0, "tick_has_owner_pid");
    if (tick) save(tick_out, tick);
    whoate_string_free(tick);

    char *why = whoate_why(engine, pid);
    CHECK(why && strstr(why, "\"owners\":[{"), "why_known_pid");
    if (why) save(why_out, why);
    whoate_string_free(why);
    char *missing = whoate_why(engine, 2147483000);
    CHECK(missing && strstr(missing, "\"error\":{\"code\":3"), "why_missing_pid_code_3");
    whoate_string_free(missing);

    if (bench > 0) {
        double begin = cpu_seconds();
        for (int i = 0; i < bench; i++) whoate_string_free(whoate_engine_tick(engine));
        double per = (cpu_seconds() - begin) / bench;
        printf("BENCH ticks=%d cpu_ms_per_tick=%.1f pct_at_1s=%.1f pct_at_2s=%.1f\n", bench, per * 1000,
               per * 100, per * 50);
    }
    whoate_engine_free(engine);
    printf("FFI_SMOKE failures=%d\n", failures);
    return failures ? 1 : 0;
}
