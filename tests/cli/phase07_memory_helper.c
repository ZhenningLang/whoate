// Independent SDK oracle and owned anonymous-memory target. Never elevated.
#include <errno.h>
#include <inttypes.h>
#include <libproc.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <unistd.h>

static int observation(pid_t pid) {
    struct proc_bsdinfo before = {0}, after = {0};
    struct rusage_info_v4 usage = {0};
    if (proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &before, sizeof before) != sizeof before)
        return errno == ESRCH ? 3 : 1;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&usage) != 0)
        return errno == ESRCH ? 3 : 1;
    if (proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &after, sizeof after) != sizeof after)
        return errno == ESRCH ? 3 : 1;
    if (before.pbi_pid != after.pbi_pid || before.pbi_uid != after.pbi_uid ||
        before.pbi_start_tvsec != after.pbi_start_tvsec || before.pbi_start_tvusec != after.pbi_start_tvusec)
        return 1;
    printf("{\"pid\":%d,\"uid\":%u,\"start_time\":%" PRIu64 ",\"resident_bytes\":%" PRIu64 "}\n",
           pid, before.pbi_uid,
           (uint64_t)before.pbi_start_tvsec * 1000000 + before.pbi_start_tvusec,
           (uint64_t)usage.ri_resident_size);
    fflush(stdout);
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 3) return 2;
    char *end = NULL;
    errno = 0;
    unsigned long long amount = strtoull(argv[2], &end, 10);
    if (errno || !amount || !end || *end || argv[2][0] == '-') return 2;
    if (!strcmp(argv[1], "--resident")) {
        if (amount > INT32_MAX) return 2;
        return observation((pid_t)amount);
    }
    if (strcmp(argv[1], "--allocate") || amount > SIZE_MAX) return 2;
    pid_t child = fork();
    if (child < 0) return 1;
    if (child > 0) _exit(0);
    if (setsid() < 0) return 1;
    alarm(120); // Owned-fixture hard lifetime even if the driver disappears.
    // Reparent to launchd so the tested owner contains this one PID only.
    while (getppid() != 1) usleep(1000);
    if (observation(getpid())) return 1; // Creation receipt before allocation.
    size_t bytes = (size_t)amount;
    volatile uint64_t *memory = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    if (memory == MAP_FAILED) return 1;
    // Nonuniform words resist compressor deduplication; this is a test fixture,
    // not an attempt to force resident <= footprint or guarantee host reclaim.
    uint64_t state = 0x9e3779b97f4a7c15ULL;
    for (size_t i = 0; i < bytes / sizeof *memory; i++) {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        memory[i] = state;
    }
    if (bytes % sizeof *memory) ((volatile char *)memory)[bytes - 1] = 1;
    if (observation(getpid())) return 1;
    for (;;) pause(); // Default SIGTERM ends only this explicitly owned target.
}
