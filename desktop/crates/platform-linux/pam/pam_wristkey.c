/* pam_wristkey.c — PAM module for WristKey BLE unlock
 * 
 * Build: gcc -shared -fPIC -o pam_wristkey.so pam_wristkey.c -lpam
 * Install: cp pam_wristkey.so /lib/security/
 * Configure: add "auth sufficient pam_wristkey.so" to /etc/pam.d/gdm-password (or lightdm, sddm, etc.)
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
#include <errno.h>

#define WRISTKEY_SOCKET "/run/wristkey/pam.sock"
#define CHALLENGE_LEN 32
#define TIMEOUT_SEC 10

static int wristkey_challenge_response(pam_handle_t *pamh, const char *user, int *retval)
{
    int sock = -1;
    struct sockaddr_un addr;
    char challenge[CHALLENGE_LEN + 1] = {0};
    char response[256] = {0};
    int result = PAM_AUTH_ERR;

    // Generate random challenge
    FILE *urandom = fopen("/dev/urandom", "rb");
    if (!urandom) {
        pam_syslog(pamh, LOG_ERR, "WristKey: cannot open /dev/urandom");
        return PAM_AUTH_ERR;
    }
    if (fread(challenge, 1, CHALLENGE_LEN, urandom) != CHALLENGE_LEN) {
        fclose(urandom);
        return PAM_AUTH_ERR;
    }
    fclose(urandom);

    // Connect to WristKey daemon via Unix socket
    sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock < 0) {
        pam_syslog(pamh, LOG_ERR, "WristKey: socket failed: %s", strerror(errno));
        return PAM_AUTH_ERR;
    }

    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, WRISTKEY_SOCKET, sizeof(addr.sun_path) - 1);

    if (connect(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        pam_syslog(pamh, LOG_ERR, "WristKey: cannot connect to daemon at %s: %s", WRISTKEY_SOCKET, strerror(errno));
        close(sock);
        return PAM_AUTH_ERR;
    }

    // Send challenge: "CHALLENGE:<user>:<hex_challenge>\n"
    char msg[512];
    snprintf(msg, sizeof(msg), "CHALLENGE:%s:", user);
    for (int i = 0; i < CHALLENGE_LEN; i++) {
        snprintf(msg + strlen(msg), sizeof(msg) - strlen(msg), "%02x", (unsigned char)challenge[i]);
    }
    strcat(msg, "\n");

    if (send(sock, msg, strlen(msg), 0) < 0) {
        pam_syslog(pamh, LOG_ERR, "WristKey: send failed: %s", strerror(errno));
        close(sock);
        return PAM_AUTH_ERR;
    }

    // Set timeout
    struct timeval tv;
    tv.tv_sec = TIMEOUT_SEC;
    tv.tv_usec = 0;
    setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));

    // Read response
    int n = recv(sock, response, sizeof(response) - 1, 0);
    close(sock);

    if (n <= 0) {
        pam_syslog(pamh, LOG_ERR, "WristKey: no response from daemon (timeout or error)");
        return PAM_AUTH_ERR;
    }
    response[n] = '\0';

    // Check response: "OK:<signature_hex>\n" or "FAIL\n"
    if (strncmp(response, "OK:", 3) == 0) {
        pam_syslog(pamh, LOG_INFO, "WristKey: authenticated user %s via BLE", user);
        result = PAM_SUCCESS;
    } else {
        pam_syslog(pamh, LOG_WARNING, "WristKey: authentication failed for %s", user);
    }

    return result;
}

PAM_EXTERN int pam_sm_authenticate(pam_handle_t *pamh, int flags,
                                   int argc, const char **argv)
{
    const char *user = NULL;
    int ret;

    ret = pam_get_user(pamh, &user, NULL);
    if (ret != PAM_SUCCESS || user == NULL) {
        pam_syslog(pamh, LOG_ERR, "WristKey: cannot determine user");
        return PAM_AUTH_ERR;
    }

    // Try WristKey first
    int wristkey_result = wristkey_challenge_response(pamh, user, &ret);
    if (wristkey_result == PAM_SUCCESS) {
        return PAM_SUCCESS;
    }

    // If WristKey fails, fall through to next module (if "sufficient" or "optional")
    // If module is marked "required", this will fail the auth stack
    return PAM_AUTH_ERR;
}

PAM_EXTERN int pam_sm_setcred(pam_handle_t *pamh, int flags,
                              int argc, const char **argv)
{
    return PAM_SUCCESS;
}

PAM_EXTERN int pam_sm_acct_mgmt(pam_handle_t *pamh, int flags,
                                int argc, const char **argv)
{
    return PAM_SUCCESS;
}
