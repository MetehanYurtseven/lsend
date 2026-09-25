# lsend

A scriptable [LocalSend](https://localsend.org) client for Linux, built the Unix way.

- **`lsendd`** runs in the background and talks to other LocalSend devices.
- **`lsendctl`** controls `lsendd`: one command per call, with plain text or JSON output.

The LocalSend protocol itself is not reimplemented: `lsendd` uses the official Rust core crate from [localsend/localsend](https://github.com/localsend/localsend) (`packages/core`).

## NixOS (Flake)

```nix
{
  inputs.lsend.url = "github:MetehanYurtseven/lsend";

  outputs = { nixpkgs, lsend, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        lsend.nixosModules.default
        {
          services.lsendd = {
            enable = true;
            openFirewall = true;
          };
        }
      ];
    };
  };
}
```

This runs `lsendd` as a systemd user service and puts `lsendctl` on the `PATH`.

| Option | Default | Description |
|---|---|---|
| `enable` | `false` | Run `lsendd` as a systemd user service. |
| `package` | this flake's package | The package providing `lsendd` and `lsendctl`. |
| `openFirewall` | `false` | Open port 53317 (TCP and UDP). |
| `accept` | `"known"` | Which requests are accepted automatically: `all`, `known`, or `none`. |
| `onText` | `"cat; echo"` | Shell command run for each received text message. |
| `systemd.target` | `"default.target"` | The user target that starts `lsendd`. |

**Example:** copy received text to the Wayland clipboard. `wl-copy` needs the graphical session:

```nix
services.lsendd = {
  enable = true;
  openFirewall = true;
  onText = lib.getExe' pkgs.wl-clipboard "wl-copy";
  systemd.target = "graphical-session.target";
};
```

## Build from source

### Nix

```sh
nix build
./result/bin/lsendd
```

### Cargo

```sh
cargo build --release
./target/release/lsendd
```

Without the NixOS module, make sure port 53317 (TCP and UDP) is open in your firewall.

## Usage

### Quick start

```sh
lsendd &                                  # or use the NixOS module
lsendctl list                             # reachable devices
lsendctl send --to iphone report.pdf      # send a file
lsendctl send --to 192.168.0.27 photos/   # send a directory, by IP
```

### Commands

| Command | Description |
|---|---|
| `lsendctl status` | Show the daemon's alias, fingerprint, and port. |
| `lsendctl list [--fingerprint]` | List reachable devices: alias, address, type (and fingerprint). |
| `lsendctl send --to <alias\|ip> <paths…>` | Send files or directories (recursively). |
| `lsendctl send --to <alias\|ip> --text <text\|->` | Send a text message. `-` reads it from stdin. |
| `lsendctl pending` | Show the request waiting for a decision. |
| `lsendctl accept [--from <fingerprint>]` | Accept the pending request. A text message is printed to stdout. |
| `lsendctl decline [--from <fingerprint>]` | Decline the pending request. |
| `lsendctl trust <alias\|ip\|fingerprint>` | Add a device to the known senders. |

`--json` works with every command and prints the daemon's response as a single line of JSON.

Devices are found passively through LocalSend's multicast discovery, there is no network scan. A device that has not announced itself yet can always be reached by its IP address.

### Receiving

`lsendd --accept` decides which requests are accepted automatically:

| Policy | Accepted automatically |
|---|---|
| `all` | every request |
| `known` (default) | requests from fingerprints in `$XDG_CONFIG_HOME/lsend/known` |
| `none` | nothing |

All other requests wait for `lsendctl accept` or `lsendctl decline` and are declined after 60 seconds. Only one request can be pending at a time.

The `known` file holds one fingerprint per line. It is read on every request, so edits take effect immediately. `lsendctl trust` appends to it:

```sh
lsendctl pending                 # iphone  192.168.0.27  A72024B1…  1 files
lsendctl trust iphone            # Trusted iphone (A72024B1…)
```

Received files are saved to `XDG_DOWNLOAD_DIR` (the current directory if it is not set). Directories keep their structure, and name collisions get a free name instead of overwriting.

### Text messages

Sending:

```sh
lsendctl send --to iphone --text "hello"
wl-paste --no-newline | lsendctl send --to iphone --text -
```

Text read from stdin is sent unchanged, nothing is trimmed.

Receiving: an automatically accepted text message is passed on the stdin of `lsendd --on-text` (run through `sh -c`). The default, `cat; echo`, prints it to the daemon's log:

```sh
lsendd --on-text wl-copy               # copy to the clipboard
lsendd --on-text 'notify-send lsend "$(cat)"'
```

A text message that waits for a decision is printed by `lsendctl accept` instead:

```sh
lsendctl accept | wl-copy
```

### Scripting

Every command exits with 0 on success. On failure, the error goes to stderr and the exit code is 1.

```sh
# Aliases of all reachable phones
lsendctl --json list | jq -r '.devices[] | select(.device_type == "mobile") | .alias'

# Accept a pending request only from a known fingerprint
lsendctl accept --from A72024B1…

# Send a screenshot to the phone
grim - > /tmp/shot.png && lsendctl send --to iphone /tmp/shot.png
```

### Files

| Path | Content |
|---|---|
| `$XDG_RUNTIME_DIR/lsend/lsendd.sock` | Socket between `lsendctl` and `lsendd` |
| `$XDG_STATE_HOME/lsend/identity.pem` | Device key and certificate, which define the fingerprint |
| `$XDG_CONFIG_HOME/lsend/known` | Fingerprints of trusted senders |

## License

[MIT](LICENSE)
