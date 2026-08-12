/*
 * PAM module for WristKey — Linux unlock via BLE
 *
 * Build: gcc -shared -fPIC -o pam_wristkey.so pam_wristkey.c -lpam
 * Install: cp pam_wristkey.so /lib/security/
 * Configure: add "auth sufficient pam_wristkey.so" to /etc/pam.d/gdm-password or /etc/pam.d/sddm
 */

#define PAM_SM_AUTH
#include <security/pam_modules.h>
#include <security/pam_ext.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>

#define SOCKET_PATH "/run/wristkeyd/unlock.sock"
#define TIMEOUT_SEC 15

PAM_EXTERN int pam_sm_authenticate(pam_handle_t *pamh, int flags,
                                   int argc, const char **argv)
{
    (void)flags;
    (void)argc;
    (void)argv;

    const char *username = NULL;
    if (pam_get_user(pamh, &username, NULL) != PAM_SUCCESS || !username) {
        username = "unknown";
    }

    pam_info(pamh, "WristKey: Bring your watch close to unlock...");

    int sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock < 0) {
        pam_error(pamh, "WristKey: socket creation failed");
        return PAM_AUTH_ERR;
    }

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, SOCKET_PATH, sizeof(addr.sun_path) - 1);

    struct timeval tv;
    tv.tv_sec = TIMEOUT_SEC;
    tv.tv_usec = 0;
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));

    if (connect(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        pam_error(pamh, "WristKey: daemon not running or not responding");
        close(sock);
        return PAM_AUTH_ERR;
    }

    char request[256];
    snprintf(request, sizeof(request), "AUTH|%s\n", username);
    if (send(sock, request, strlen(request), 0) < 0) {
        pam_error(pamh, "WristKey: failed to send request");
        close(sock);
        return PAM_AUTH_ERR;
    }

    char response[64];
    ssize_t n = recv(sock, response, sizeof(response) - 1, 0);
    close(sock);

    if (n <= 0) {
        pam_error(pamh, "WristKey: no response from daemon (timeout)");
        return PAM_AUTH_ERR;
    }
    response[n] = '\0';

    if (strncmp(response, "OK", 2) == 0) {
        pam_info(pamh, "WristKey: unlocked via watch");
        return PAM_SUCCESS;
    } else {
        pam_error(pamh, "WristKey: authentication failed");
        return PAM_AUTH_ERR;
    }
}

PAM_EXTERN int pam_sm_setcred(pam_handle_t *pamh, int flags,
                              int argc, const char **argv)
{
    (void)pamh;
    (void)flags;
    (void)argc;
    (void)argv;
    return PAM_SUCCESS;
}

PAM_EXTERN int pam_sm_acct_mgmt(pam_handle_t *pamh, int flags,
                                int argc, const char **argv)
{
    (void)pamh;
    (void)flags;
    (void)argc;
    (void)argv;
    return PAM_SUCCESS;
}
