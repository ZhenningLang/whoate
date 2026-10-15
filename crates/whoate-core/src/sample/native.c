/* SDK-owned structures stay in C; Rust sees only these small value records.
 * This avoids guessed offsets in kinfo_proc and socket_info unions. */
#include <sys/types.h>
#include <sys/sysctl.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#include <arpa/inet.h>
#include <sys/resource.h>
#include <libproc.h>
#include <mach/mach.h>
#include <mach/mach_host.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

struct capture_identity {
    int32_t pid, ppid;
    uint32_t uid;
    uint32_t reserved;
    uint64_t start_us;
    char comm[17];
};

int capture_identities(struct capture_identity **out, size_t *count) {
    int mib[] = {CTL_KERN, KERN_PROC, KERN_PROC_ALL, 0};
    size_t bytes = 0;
    if (sysctl(mib, 4, NULL, &bytes, NULL, 0) != 0) return errno;
    /* Additional space tolerates processes launched between the two calls.
     * ENOMEM is returned, not a partial success, if the list still outgrows it. */
    bytes += bytes / 4 + sizeof(struct kinfo_proc);
    struct kinfo_proc *rows = malloc(bytes);
    if (!rows) return ENOMEM;
    if (sysctl(mib, 4, rows, &bytes, NULL, 0) != 0) {
        int error = errno; free(rows); return error;
    }
    if (bytes % sizeof(*rows)) { free(rows); return EINVAL; }
    size_t n = bytes / sizeof(*rows);
    struct capture_identity *result = calloc(n, sizeof(*result));
    if (!result) { free(rows); return ENOMEM; }
    for (size_t i = 0; i < n; ++i) {
        result[i].pid = rows[i].kp_proc.p_pid;
        result[i].ppid = rows[i].kp_eproc.e_ppid;
        result[i].uid = rows[i].kp_eproc.e_ucred.cr_uid;
        result[i].start_us = (uint64_t)rows[i].kp_proc.p_starttime.tv_sec * 1000000
                            + rows[i].kp_proc.p_starttime.tv_usec;
        memcpy(result[i].comm, rows[i].kp_proc.p_comm, 16);
    }
    free(rows); *out = result; *count = n; return 0;
}

struct capture_host {
    uint64_t total, internal_pages, purgeable_pages, wired_pages, compressed_pages;
    uint64_t page_size, swap_used;
    int32_t pressure;
};

int capture_host(struct capture_host *out) {
    size_t size = sizeof(out->total);
    if (sysctlbyname("hw.memsize", &out->total, &size, NULL, 0)) return errno;
    mach_port_t host = mach_host_self();
    vm_size_t page_size;
    vm_statistics64_data_t stats;
    mach_msg_type_number_t count = HOST_VM_INFO64_COUNT;
    kern_return_t status = host_page_size(host, &page_size);
    if (status == KERN_SUCCESS)
        status = host_statistics64(host, HOST_VM_INFO64, (host_info64_t)&stats, &count);
    mach_port_deallocate(mach_task_self(), host);
    if (status != KERN_SUCCESS) return EIO;
    out->page_size = page_size;
    out->internal_pages = stats.internal_page_count;
    out->purgeable_pages = stats.purgeable_count;
    out->wired_pages = stats.wire_count;
    out->compressed_pages = stats.compressor_page_count;
    struct xsw_usage swap;
    size = sizeof(swap);
    if (sysctlbyname("vm.swapusage", &swap, &size, NULL, 0)) return errno;
    out->swap_used = swap.xsu_used;
    size = sizeof(out->pressure);
    if (sysctlbyname("kern.memorystatus_vm_pressure_level", &out->pressure, &size, NULL, 0)) return errno;
    return 0;
}

struct capture_socket { uint64_t socket_id; uint16_t port; uint16_t state; };
int capture_sockets(int pid, struct capture_socket **out, size_t *count) {
    int bytes = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, NULL, 0);
    if (bytes <= 0) return errno ? errno : ESRCH;
    bytes += bytes / 4 + (int)sizeof(struct proc_fdinfo);
    struct proc_fdinfo *fds = malloc((size_t)bytes);
    if (!fds) return ENOMEM;
    int used = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, bytes);
    if (used <= 0 || used == bytes || used % sizeof(*fds)) {
        int error = used <= 0 ? errno : EOVERFLOW; free(fds); return error ? error : EIO;
    }
    size_t n = (size_t)used / sizeof(*fds), found = 0;
    struct capture_socket *result = calloc(n, sizeof(*result));
    if (!result) { free(fds); return ENOMEM; }
    for (size_t i = 0; i < n; ++i) {
        if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) continue;
        struct socket_fdinfo socket;
        int got = proc_pidfdinfo(pid, fds[i].proc_fd, PROC_PIDFDSOCKETINFO, &socket, sizeof(socket));
        if (got != sizeof(socket)) {
            int error = errno ? errno : EIO; free(fds); free(result); return error;
        }
        if (socket.psi.soi_kind != SOCKINFO_TCP ||
            (socket.psi.soi_family != AF_INET && socket.psi.soi_family != AF_INET6)) continue;
        result[found].socket_id = socket.psi.soi_so;
        result[found].port = ntohs((uint16_t)socket.psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport);
        result[found].state = (uint16_t)socket.psi.soi_proto.pri_tcp.tcpsi_state;
        ++found;
    }
    free(fds); *out = result; *count = found; return 0;
}
