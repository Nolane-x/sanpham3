# G8 USB Bulk Physical Court

Status: **authored, physical evidence pending**.

This court validates the opportunistic USB path without changing the product
constraint that no external hardware is mandatory.

## Required setup

- Android device running a build that exposes the USB court UI/harness;
- an already-connected user-owned/project peer that exposes one USB interface
  with bulk IN and bulk OUT endpoints;
- Android USB permission granted explicitly by the user;
- the same 32-byte project peer key provisioned on both ends;
- distinct node IDs.

The peer may be a development computer/device used for validation. Passing this
court does not make that hardware a product requirement.

## Procedure

1. Scan `AndroidUsbBulkDataPath`.
2. Record candidate metadata and `permissionGranted`.
3. Select one bulk-duplex candidate.
4. Open and claim the interface.
5. Assign one end the peer-session client role and the other the server role.
6. Run the standard G8 encrypted challenge/ACK exchange.
7. Send at least 100 authenticated application messages in both directions.
8. Record useful bytes and elapsed time.
9. Disconnect/reconnect once and repeat the authenticated handshake.
10. Attempt one run with USB permission absent and require a typed
    `USB_PERMISSION_REQUIRED` failure rather than fallback trust.

## Required evidence

Record:

- git commit;
- Android API/device model;
- USB VID/PID;
- interface ID;
- endpoint addresses/max-packet sizes;
- permission state;
- local/peer node IDs;
- authenticated peer ID;
- G8 challenge/ACK result;
- setup/handshake latency;
- useful bytes and elapsed time;
- detach/reconnect outcome;
- failure reason for the no-permission run;
- SHA-256 manifest over evidence files.

Do not record the project peer key.

## PASS conditions

A physical PASS requires all of:

- the selected bulk IN/OUT interface is opened through stock Android APIs;
- peer-session authentication succeeds;
- peer node ID matches the expected peer;
- G8 challenge/ACK succeeds;
- bidirectional authenticated application messages succeed;
- no-permission run fails closed;
- reconnect requires a new authenticated peer session;
- evidence contains real measurements.

## Not proven by Android CI

The current Android CI can prove compilation and pure endpoint-selection tests.

It cannot prove:

- a cable enumerates;
- USB permission UI works;
- real bulk endpoints transfer bytes;
- physical G8 succeeds;
- useful throughput or energy.

Until this court is executed on real devices, those claims remain open.
