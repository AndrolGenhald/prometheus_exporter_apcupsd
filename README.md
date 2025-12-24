# Prometheus Exporter for apcupsd

A Prometheus exporter that pulls data from apcupsd.

I went through the manual and source code of apcupsd to see what data the NIS service presents (mostly in apcstatus.c) and created metrics for
everything. The examples in the apcupsd source are included as test files (although they've been modified, as they are quite old and some fields have
been added, removed, and changed in apcupsd).

## Configuration

Configuration is read from `/etc/prometheus/apcupsd_exporter_config.yaml` by default, a different filepath can be specified with the `-c` flag.
Configuration is limited to what I personally needed, so changing the apcupsd NIS server host (localhost) or port (3551) from their defaults isn't
currently supported. If you want to be able to change those, or to be able to run multiple instances of the exporter to export multiple UPSes on a
single host, feel free to open an issue on [GitHub](https://github.com/AndrolGenhald/prometheus_exporter_apcupsd).

### Example

```
# Exporter web server listening address; default 127.0.0.1:9175
address: 0.0.0.0:9175
# HTTP authentication type; default !None
authorization: !Basic "secret-password"
# TLS options; default none
tls_options:
  # TLS certificate used to serve HTTPS; required
  certificate_chain_file: /path/to/certificate.crt
  # Private key used for TLS when serving HTTPS; required
  key_file: /path/to/key.key
  # CA certificate used to sign client certificates when doing mutual TLS; optional
  client_certificate_ca_file: /path/to/ca-certificate.crt
# Timeout in seconds when requesting data from apcupsd; default 1
apcaccess_timeout: 2.5
```

## Why not https://github.com/mdlayher/apcupsd_exporter or https://github.com/io-developer/prom-apcupsd-exporter?

The io-developer implementation includes a websocket server that I don't want and which can't be disabled. It also makes some (in my opinion)
questionable choices in the way that it exposes certain metrics like cable, self test result, etc, by mapping strings to integers arbitrarily.

The mdlayher implementation only exposes a small subset of the data from apcupsd, and returns "0" for missing data (including when apcupsd isn't
running), which pollutes prometheus with incorrect data.

Neither implementation supports HTTPS or any type of authentication, which is a hard requirement for me, with Basic auth over HTTPS being bare minimum
and mutual TLS being preferred.

Also I just wanted to have some fun writing my own implementation to explore the problem space.
