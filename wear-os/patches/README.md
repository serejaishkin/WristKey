# Wear OS GATT fix

This patch is intentionally provided as a replacement script instead of replacing `WristKeyBleService.kt` wholesale.

## Apply

From the repository root:

```bash
bash wear-os/patches/fix-gatt-after-forget.sh
```

The script changes only `forgetDevice()`:

- clears the paired device;
- stops the old GATT server;
- starts a fresh GATT server;
- resets the advertisement PIN.

It aborts without changes if the expected source block is not found exactly once.

## Pairing safety

Do not manually modify the pairing protocol while applying this patch. In particular, leave the challenge/response flow, public-key characteristic, `confirmPairing()`, pairing-key handling, and GATT characteristic UUIDs unchanged.
