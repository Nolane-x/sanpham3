# Exact Tiny HTTPS Path Probe

The recovery engine can probe an HTTPS endpoint through each individual Linux interface or Windows source address.

## Why the target is explicit

The repository does not hard-code a third-party probe service.

A hard-coded endpoint would create a privacy/dependency boundary and could turn one remote outage into a false 'no Internet' result.

Instead, operators configure a literal destination IP plus TLS server name.

## CLI environment

Set:

```text
SP3_HTTPS_PROBE_ADDR=<literal-ip>:443
SP3_HTTPS_PROBE_NAME=<tls-server-name>
SP3_HTTPS_PROBE_PATH=/
SP3_HTTPS_PROBE_MAX_BYTES=1024
```

`SP3_HTTPS_PROBE_ADDR` must be a literal IP:port. This is deliberate: the HTTPS test must not silently perform DNS through a different interface.

`SP3_HTTPS_PROBE_PATH` defaults to `/`.

`SP3_HTTPS_PROBE_MAX_BYTES` defaults to 1024.

If neither address nor name is configured, the Ledger records TinyHttps as Unsupported rather than claiming that HTTPS failed.

If only one of address/name is configured, `host-probe-cli` exits with a configuration error.

## Linux

Linux uses interface-bound sockets (`SO_BINDTODEVICE`) before TCP/TLS establishment.

## Windows

Windows selects a local unicast address that matches the target address family and binds the TCP socket to that source IP before TLS establishment.

## Evidence

A successful attempt validates:

- exact path binding;
- TCP connectivity;
- TLS certificate validation for the configured server name;
- an HTTP response with a valid status line;
- response bytes;
- repeated-attempt loss/intermittency;
- useful-byte rate over the measured attempt series.

This is stronger evidence than a route flag or DNS-only success.

## Remaining limitations

- one configured target can fail independently of the Internet;
- multiple independent targets should eventually be supported by product configuration;
- a successful tiny HEAD request does not prove bulk throughput;
- physical weak-link behavior still requires field testing.