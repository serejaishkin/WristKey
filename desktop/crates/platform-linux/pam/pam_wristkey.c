/* pam_wristkey.c — PAM module for WristKey BLE unlock
 *
 * Build: gcc -shared -fPIC -o pam_wristkey.so pam_wristkey.c -lpam
 * Install: cp pam_wristkey.so /lib/security/
 * Configure: add "auth sufficient pam_wristkey.so" to /etc/pam.d/gdm-password (or lightdm, sddm, etc.)
 */

#define PAM_SM_AUTH
#include <security/pam_modules.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/types.h>
#include <pwd.h>
#include <time.h>
#include <errno.h>

#define TIMEOUT_MS 5000

static int check_last_auth(pam_handle_t *pamh, const char *user)
{
    const char *home = NULL;
    struct passwd *pw = getpwnam(user);
    if (pw != NULL) {
        home = pw->pw_dir;
    } else {
        home = getenv("HOME");
    }
    if (home == NULL) {
        pam_syslog(pamh, LOG_ERR, "WristKey: cannot determine home directory for %s", user);
        return PAM_AUTH_ERR;
    }

    char path[512];
    snprintf(path, sizeof(path), "%s/.wristkey/.last_auth", home);

    FILE *f = fopen(path, "r");
    if (f == NULL) {
        pam_syslog(pamh, LOG_INFO, "WristKey: no auth proof for %s", user);
        return PAM_AUTH_ERR;
    }

    char line1[64] = {0};
    char line2[256] = {0};
    if (fgets(line1, sizeof(line1), f) == NULL || fgets(line2, sizeof(line2), f) == NULL) {
        fclose(f);
        return PAM_AUTH_ERR;
    }
    fclose(f);

    // Remove trailing newline
    line1[strcspn(line1, "\n")] = '\0';
    line2[strcspn(line2, "\n")] = '\0';

    unsigned long timestamp = strtoul(line1, NULL, 10);
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    unsigned long now_ms = (unsigned long)(now.tv_sec * 1000 + now.tv_nsec / 1000000);

    if (now_ms < timestamp || (now_ms - timestamp) > TIMEOUT_MS) {
        pam_syslog(pamh, LOG_WARNING, "WristKey: auth proof expired for %s (delta=%lu ms)", user, now_ms - timestamp);
        return PAM_AUTH_ERR;
    }

    // Verify username matches
    if (strcmp(line2, user) != 0) {
        pam_syslog(pamh, LOG_WARNING, "WristKey: auth proof username mismatch: expected %s, got %s", user, line2);
        return PAM_AUTH_ERR;
    }

    // Verify file ownership matches invoking user
    struct stat st;
    if (stat(path, &st) == 0) {
        if (pw != NULL && st.st_uid != pw->pw_uid) {
            pam_syslog(pamh, LOG_WARNING, "WristKey: auth proof file ownership mismatch for %s", user);
            return PAM_AUTH_ERR;
        }
    }

    // Success! Delete proof to prevent replay
    if (unlink(path) != 0) {
        pam_syslog(pamh, LOG_WARNING, "WristKey: failed to delete auth proof: %s", strerror(errno));
    }

    pam_syslog(pamh, LOG_INFO, "WristKey: authenticated user %s via BLE", user);
    return PAM_SUCCESS;
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

    return check_last_auth(pamh, user);
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
