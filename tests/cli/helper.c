#include <libproc.h>
#include <sys/sysctl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "identity") == 0) {
        int pid = atoi(argv[2]);
        struct kinfo_proc info = {0};
        int mib[] = { CTL_KERN, KERN_PROC, KERN_PROC_PID, pid };
        size_t size = sizeof(info);
        /* Use the same SDK-defined identity source as P1, including system PIDs.
         * No guessed layouts and no signal or privileged helper. */
        if (pid <= 0 || sysctl(mib, 4, &info, &size, NULL, 0) != 0 || size != sizeof(info)) return 1;
        printf("{\"pid\":%d,\"uid\":%u,\"start_time\":%llu}\n", pid, info.kp_eproc.e_ucred.cr_uid,
               (unsigned long long) info.kp_proc.p_starttime.tv_sec * 1000000 + info.kp_proc.p_starttime.tv_usec);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "ignore-term") == 0) signal(SIGTERM, SIG_IGN);
    printf("%d\n", getpid());
    fflush(stdout);
    for (;;) pause();
}
