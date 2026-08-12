# Build PAM module for WristKey
CC = gcc
CFLAGS = -Wall -fPIC -shared
LDFLAGS = -lpam

all: pam_wristkey.so

pam_wristkey.so: pam_wristkey.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

install: pam_wristkey.so
	install -m 644 pam_wristkey.so /lib/security/
	@echo "Add to /etc/pam.d/gdm-password or /etc/pam.d/sddm:"
	@echo "  auth sufficient pam_wristkey.so"

clean:
	rm -f pam_wristkey.so
