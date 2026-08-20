# WristKey Reference Projects

This document lists external projects used as architectural and UX references for WristKey.

They are references only. WristKey does not copy their authentication protocol, keys, security model, or implementation.

## PC Bio Unlock

https://github.com/MeisApps/pcbu-desktop

Useful references:
- desktop authentication architecture
- daemon/service separation
- OS login integration
- pairing and device management
- unlock flow
- fallback authentication
- cross-platform architecture

WristKey-specific differences:
- Galaxy Watch is the primary authentication device
- BLE challenge/response is WristKey-owned
- the PC retains the user's credential/password
- the watch acts as a physical confirmation factor

## ProximityLock

https://proximitylock.app/

Useful references:
- proximity-based locking and unlocking
- presence/absence state handling
- background operation
- lock/unlock UX
- user feedback

WristKey-specific differences:
- cryptographic authentication rather than proximity alone
- explicit watch confirmation for unlock
- OS-specific secure credential integration

## Security boundary

Reference projects must not become protocol dependencies. The WristKey pairing protocol is considered a stable/frozen interface while cross-platform desktop integration is being completed. Changes to challenge/response, key exchange, UUIDs, or pairing semantics require a separate security review.
