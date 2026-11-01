// Runs a command and reports on stderr:
//   peakrss <max_rss_kb> <wall_ns> <user_us> <sys_us> <voluntary_csw> <involuntary_csw>
// A child's RSS high-water mark starts at its parent's size when forked, so launching from
// Python would floor every reading at ~14 MB; this launcher is small enough not to matter.
// The command's own stderr goes to /dev/null so the report line is the only stderr output.
#include <fcntl.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: peakrss CMD [ARGS...]\n");
        return 2;
    }
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    pid_t pid = fork();
    if (pid < 0) {
        perror("peakrss: fork");
        return 2;
    }
    if (pid == 0) {
        int devnull = open("/dev/null", O_WRONLY);
        dup2(devnull, 2);
        execvp(argv[1], argv + 1);
        _exit(127);
    }
    int status;
    struct rusage ru;
    wait4(pid, &status, 0, &ru);
    clock_gettime(CLOCK_MONOTONIC, &end);
    long long ns = (end.tv_sec - start.tv_sec) * 1000000000LL + (end.tv_nsec - start.tv_nsec);
    fprintf(stderr, "peakrss %ld %lld %ld %ld %ld %ld\n", ru.ru_maxrss, ns,
            ru.ru_utime.tv_sec * 1000000L + ru.ru_utime.tv_usec,
            ru.ru_stime.tv_sec * 1000000L + ru.ru_stime.tv_usec, ru.ru_nvcsw, ru.ru_nivcsw);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
