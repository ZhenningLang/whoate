/* Negative-control wrapper for tests/gui/ls_negative_control.py.
 *
 * Launched with `open -n` as a checked-in accessory app, it plays the part of
 * the terminal in the 2026-10-20 incident: its coalition is tracked by
 * LaunchServices, and the bare app runs inside it as a child. It also starts
 * a `sleep` of its own. After the app exits, this process stays alive until
 * the test ends it, so the app's entry has living subordinates.
 *
 *   wrapper <run dir> <app executable>
 * writes <run dir>/{wrapper,sleep,app}.pid and the app's output to app.stdout. */
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

extern int NSApplicationLoad(void);
extern char **environ;

static void record(const char *dir, const char *name, pid_t pid) {
    char path[4096];
    snprintf(path, sizeof path, "%s/%s.pid", dir, name);
    FILE *f = fopen(path, "w");
    if (f) { fprintf(f, "%d\n", pid); fclose(f); }
}

int main(int argc, char **argv) {
    if (argc != 3) return 2;
    NSApplicationLoad();
    record(argv[1], "wrapper", getpid());
    pid_t sleeper, app;
    char *sleep_argv[] = {"/bin/sleep", "600", NULL};
    if (posix_spawn(&sleeper, "/bin/sleep", NULL, NULL, sleep_argv, environ)) return 1;
    record(argv[1], "sleep", sleeper);
    char out[4096];
    snprintf(out, sizeof out, "%s/app.stdout", argv[1]);
    posix_spawn_file_actions_t files;
    posix_spawn_file_actions_init(&files);
    posix_spawn_file_actions_addopen(&files, 1, out, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    posix_spawn_file_actions_adddup2(&files, 1, 2);
    char *app_argv[] = {argv[2], NULL};
    if (posix_spawn(&app, argv[2], &files, NULL, app_argv, environ)) return 1;
    record(argv[1], "app", app);
    waitpid(app, NULL, 0);
    for (;;) pause();
}
