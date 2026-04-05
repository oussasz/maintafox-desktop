# Device Trust Contracts

Reference for the trusted-device model, IPC commands, and offline policy enforcement.

## Device Identity Model

The device fingerprint is derived as:
```
SHA-256( machine_id : hostname : os_type )
```
- **Stable:** survives reboots, application updates, and login user changes
- **Hashed:** raw machine ID is not stored; the hash cannot be reversed to get hardware details
- **Scope:** identifies the OS installation, not the logged-in user account

The device secret (32-byte random) is stored in the OS keyring (Windows Credential
Manager, macOS Keychain, Linux Secret Service). It is used as an HMAC material for
the Phase 3 sync device challenge. It is generated once on first launch and never changes.

## Trust Lifecycle

```
[first online login]
  -> credentials accepted
  -> register_device_trust(user_id, fingerprint)
  -> trusted_devices row created

[subsequent online login]
  -> register_device_trust() called -> updates last_seen_at

[offline login attempt]
  -> check_offline_access(user_id, fingerprint)
  -> if no trust record -> DENIED
  -> if trust revoked -> DENIED
  -> if last_seen_at + grace_hours < now -> DENIED (grace expired)
  -> else -> ALLOWED

[admin revokes device]
  -> revoke_device_trust(device_id)
  -> is_revoked = 1
  -> offline login denied for this device+user combination
  -> online login still allowed (machine is not blocked at OS level)
```

## Offline Grace Policy

The grace window is configurable per tenant:

| Config key | Default | Maximum |
|-----------|---------|---------|
| `offline_grace_hours` | 72 (3 days) | 168 (7 days) |

The maximum of 168 hours is enforced at the application level regardless of what value
is stored in `system_config`. This prevents a database edit from extending the grace
window beyond policy limits -- even if the database is decrypted by an attacker.

## IPC Commands

### get_device_trust_status
```
Requires: authenticated session
Returns:  DeviceTrustStatus
```
Returns the trust status of the current device for the logged-in user, including whether
offline access is currently allowed and how many hours remain.

### revoke_device_trust
```
Requires: authenticated session + adm.users permission (enforced SP04-F03)
Payload:  { device_id: string }
Returns:  null
Errors:   NOT_FOUND if device_id does not exist or is already revoked
```
Revokes offline trust for a specific device. The device can still log in online. Used
when a laptop is lost or stolen to prevent offline access with cached credentials.

## Security Notes

1. The `is_network_available()` check is a **local best-effort** signal, not a VPS
   round-trip. An attacker with local OS access could spoof the network state. The
   device trust model relies on the OS keyring secret as the durable binding material;
   the network check prevents accidental offline registration on first login only.

2. Device trust revocation takes effect on the NEXT login attempt. An active session
   is not terminated by revocation -- use the session manager `clear_session()` for that.

3. The `offline_grace_hours` cap of 168 hours is enforced at the application code level
   and is not overridable by tenant configuration or database edit. Any PR that raises
   this cap to above 168 or makes it unlimited must include a security review sign-off.
