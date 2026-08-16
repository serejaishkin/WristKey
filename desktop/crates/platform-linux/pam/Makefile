# Makefile for pam_wristkey.so

CC = gcc
CFLAGS = -shared -fPIC -Wall -Wextra
LDFLAGS = -lpam

TARGET = pam_wristkey.so
SRC = pam_wristkey.c

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

clean:
	rm -f $(TARGET)

install: $(TARGET)
	cp $(TARGET) /lib/security/
	@echo "Installed to /lib/security/$(TARGET)"
	@echo "Add 'auth sufficient pam_wristkey.so' to /etc/pam.d/gdm-password (or your display manager)"

.PHONY: all clean install
