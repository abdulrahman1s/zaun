//! Host filesystem trees for the host probes (HOST-004, HOST-006, HOST-007): distribution
//! layouts, input devices with udev data, and umu installations. Each builder writes into a
//! [`MemFs`]; values follow captured systems where noted. (`HostFacts` presets live in
//! `hosts.rs`.)

use std::path::Path;

use easyplay_core::fsx::Fs;

use crate::MemFs;

/// The `input` group of the fixtures.
pub const INPUT_GID: u32 = 97;
/// The fixture user.
pub const USER_UID: u32 = 1000;

fn exe(fs: &MemFs, path: &str, content: &str) {
    fs.file(path, content);
    fs.set_mode(Path::new(path), 0o755).unwrap();
}

fn os_release(fs: &MemFs, id: &str, like: &str, pretty: &str) {
    let like = if like.is_empty() {
        String::new()
    } else {
        format!("ID_LIKE={like}\n")
    };
    fs.file(
        "/usr/lib/os-release",
        format!("NAME=\"{pretty}\"\nPRETTY_NAME=\"{pretty}\"\nID={id}\n{like}"),
    );
    fs.link("/etc/os-release", "../usr/lib/os-release");
}

fn merged_usr(fs: &MemFs, lib64: &str) {
    fs.dir("/usr/bin").dir("/usr/lib").dir("/usr/lib64");
    fs.link("/bin", "usr/bin")
        .link("/sbin", "usr/bin")
        .link("/lib", "usr/lib")
        .link("/lib64", lib64);
}

/// Arch Linux: merged `/usr`, systemd-resolved stub, fakeroot in `ld.so.conf.d`.
pub fn arch(fs: &MemFs) {
    merged_usr(fs, "usr/lib");
    os_release(fs, "arch", "", "Arch Linux");
    exe(fs, "/usr/bin/bwrap", "\x7fELF");
    exe(fs, "/usr/bin/true", "\x7fELF");
    exe(fs, "/usr/bin/python3", "\x7fELF");
    fs.dir("/opt");
    fs.file(
        "/etc/ld.so.conf",
        "# Dynamic linker\ninclude /etc/ld.so.conf.d/*.conf\n",
    )
    .file("/etc/ld.so.conf.d/fakeroot.conf", "/usr/lib/libfakeroot\n")
    .file("/etc/ld.so.cache", "cache")
    .file("/etc/hosts", "127.0.0.1 localhost\n")
    .dir("/etc/fonts")
    .dir("/etc/vulkan")
    .file(
        "/run/systemd/resolve/stub-resolv.conf",
        "nameserver 127.0.0.53\n",
    )
    .link(
        "/etc/resolv.conf",
        "../run/systemd/resolve/stub-resolv.conf",
    )
    .file("/usr/share/zoneinfo/Europe/Berlin", "TZif")
    .link("/etc/localtime", "../usr/share/zoneinfo/Europe/Berlin");
}

/// Debian 12: merged `/usr` including `lib32`/`libx32`, multiarch library directories,
/// alternatives, a regular `resolv.conf`.
pub fn debian(fs: &MemFs) {
    merged_usr(fs, "usr/lib64");
    fs.dir("/usr/lib32").dir("/usr/libx32");
    fs.link("/lib32", "usr/lib32").link("/libx32", "usr/libx32");
    os_release(fs, "debian", "", "Debian GNU/Linux 12 (bookworm)");
    exe(fs, "/usr/bin/bwrap", "\x7fELF");
    fs.file("/etc/ld.so.conf", "include /etc/ld.so.conf.d/*.conf\n")
        .file(
            "/etc/ld.so.conf.d/x86_64-linux-gnu.conf",
            "# Multiarch support\n/usr/local/lib/x86_64-linux-gnu\n/lib/x86_64-linux-gnu\n/usr/lib/x86_64-linux-gnu\n",
        )
        .file("/etc/ld.so.conf.d/libc.conf", "/usr/local/lib\n")
        .dir("/usr/local/lib/x86_64-linux-gnu")
        .dir("/usr/lib/x86_64-linux-gnu")
        .dir("/etc/alternatives")
        .file("/etc/resolv.conf", "nameserver 192.168.1.1\n")
        .file("/etc/hosts", "127.0.0.1 localhost\n")
        .file("/etc/timezone", "Europe/Berlin\n");
}

/// Fedora: merged `/usr`, `/lib64`, NetworkManager `resolv.conf`, a vendor library root
/// outside `/usr` registered in `ld.so.conf.d` (the "extra library root" case).
pub fn fedora(fs: &MemFs) {
    merged_usr(fs, "usr/lib64");
    os_release(fs, "fedora", "", "Fedora Linux 43");
    exe(fs, "/usr/bin/bwrap", "\x7fELF");
    fs.dir("/opt/vendor/lib")
        .dir("/srv/gpu/lib64")
        .file("/etc/ld.so.conf", "include ld.so.conf.d/*.conf\n")
        .file(
            "/etc/ld.so.conf.d/vendor.conf",
            "/opt/vendor/lib\n/srv/gpu/lib64\n/does/not/exist\n",
        )
        .file("/etc/ld.so.conf.d/llvm.conf", "/usr/lib64/llvm\n")
        .file("/run/NetworkManager/resolv.conf", "nameserver 10.0.0.1\n")
        .link("/etc/resolv.conf", "../run/NetworkManager/resolv.conf")
        .dir("/etc/pki/tls")
        .file("/etc/hosts", "127.0.0.1 localhost\n");
}

/// NixOS, as on the M0 test host: almost nothing in `/usr`, `/etc` entries symlinked through
/// `/etc/static`, `/run/current-system`, `/run/opengl-driver`, a per-user profile.
pub fn nixos(fs: &MemFs) {
    let sys = "/nix/store/aaaa-nixos-system-26.11";
    let etc = "/nix/store/bbbb-etc/etc";
    fs.dir(format!("{sys}/sw/bin"))
        .link("/run/current-system", sys)
        .link("/usr/bin/env", "/nix/store/cccc-coreutils-9.12/bin/env")
        .file("/nix/store/cccc-coreutils-9.12/bin/env", "\x7fELF")
        .link("/bin/sh", "/nix/store/dddd-bash-5.3/bin/sh")
        .file("/nix/store/dddd-bash-5.3/bin/sh", "\x7fELF")
        .dir("/nix/store/eeee-mesa/lib")
        .link("/run/opengl-driver", "/nix/store/eeee-mesa")
        .link("/etc/static", etc)
        .file(
            format!("{etc}/os-release"),
            "NAME=NixOS\nID=nixos\nPRETTY_NAME=\"NixOS 26.11 (Zokor)\"\n",
        )
        .link("/etc/os-release", "/etc/static/os-release")
        .dir("/nix/store/ffff-tzdata/share/zoneinfo/Africa")
        .file("/nix/store/ffff-tzdata/share/zoneinfo/Africa/Cairo", "TZif")
        .link(
            format!("{etc}/zoneinfo"),
            "/nix/store/ffff-tzdata/share/zoneinfo",
        )
        .link("/etc/zoneinfo", "/etc/static/zoneinfo")
        .link("/etc/localtime", "/etc/zoneinfo/Africa/Cairo")
        .file(format!("{etc}/nsswitch.conf"), "passwd: files\n")
        .link("/etc/nsswitch.conf", "/etc/static/nsswitch.conf")
        .file(format!("{etc}/resolv.conf"), "nameserver 1.1.1.1\n")
        .link("/etc/resolv.conf", "/etc/static/resolv.conf")
        .file("/etc/passwd", "sam:x:1000:100::/home/sam:/bin/sh\n")
        .file("/etc/group", "users:x:100:\n")
        .file(format!("{etc}/hosts"), "127.0.0.1 localhost\n")
        .link("/etc/hosts", "/etc/static/hosts")
        .dir("/etc/profiles/per-user/sam/bin");
    exe(fs, &format!("{sys}/sw/bin/true"), "\x7fELF");
    let bwrap = "/nix/store/gggg-bubblewrap-0.12.0/bin/bwrap";
    exe(fs, bwrap, "\x7fELF");
    fs.link(format!("{sys}/sw/bin/bwrap"), bwrap);
}

/// Input devices: a keyboard with a separate media-key node, a mouse, an Xbox pad (xpad,
/// `event21` + `js0`), a DualSense (`event22` + `js1`, touchpad `event23`, `hidraw4`).
/// Every node is `root:input 0660`; joystick nodes additionally belong to the user, standing
/// in for logind's `uaccess` ACL.
pub fn input_devices(fs: &MemFs) {
    let usb = "/sys/devices/pci0000:00/0000:00:14.0/usb1";
    let ds = format!("{usb}/1-3/1-3:1.3/0003:054C:0CE6.0005");
    // (node, major, minor, sysfs input device dir, udev properties, user-accessible)
    let nodes: Vec<(&str, u64, u64, String, &str, bool)> = vec![
        (
            "event3",
            13,
            67,
            format!("{usb}/1-1/1-1:1.0/0003:046D:C31C.0001/input/input3"),
            "E:ID_INPUT=1\nE:ID_INPUT_KEY=1\nE:ID_INPUT_KEYBOARD=1\n",
            false,
        ),
        (
            "event4",
            13,
            68,
            format!("{usb}/1-1/1-1:1.1/0003:046D:C31C.0002/input/input4"),
            "E:ID_INPUT=1\nE:ID_INPUT_KEY=1\n",
            false,
        ),
        (
            "event5",
            13,
            69,
            format!("{usb}/1-4/1-4:1.0/0003:1532:0084.0003/input/input5"),
            "E:ID_INPUT=1\nE:ID_INPUT_MOUSE=1\n",
            false,
        ),
        (
            "event21",
            13,
            85,
            format!("{usb}/1-2/1-2:1.0/input/input21"),
            "E:ID_INPUT=1\nE:ID_INPUT_JOYSTICK=1\nE:ID_BUS=usb\n",
            true,
        ),
        (
            "js0",
            13,
            0,
            format!("{usb}/1-2/1-2:1.0/input/input21"),
            "E:ID_INPUT=1\nE:ID_INPUT_JOYSTICK=1\n",
            true,
        ),
        (
            "event22",
            13,
            86,
            format!("{ds}/input/input22"),
            "E:ID_INPUT=1\nE:ID_INPUT_JOYSTICK=1\n",
            true,
        ),
        (
            "js1",
            13,
            1,
            format!("{ds}/input/input22"),
            "E:ID_INPUT=1\nE:ID_INPUT_JOYSTICK=1\n",
            true,
        ),
        (
            "event23",
            13,
            87,
            format!("{ds}/input/input23"),
            "E:ID_INPUT=1\nE:ID_INPUT_TOUCHPAD=1\n",
            false,
        ),
    ];
    for (name, major, minor, dir, props, user) in nodes {
        let dev = format!("/dev/input/{name}");
        fs.char_device(&dev, (major << 8) | minor);
        fs.chown(&dev, if user { USER_UID } else { 0 }, INPUT_GID);
        fs.file(format!("/run/udev/data/c{major}:{minor}"), props);
        fs.dir(format!("{dir}/{name}"));
        fs.link(format!("/sys/class/input/{name}"), format!("{dir}/{name}"));
    }
    fs.char_device("/dev/hidraw4", (240 << 8) | 4);
    fs.chown("/dev/hidraw4", USER_UID, INPUT_GID);
    fs.dir(format!("{ds}/hidraw/hidraw4"));
    fs.link("/sys/class/hidraw/hidraw4/device", ds.as_str());
}

/// umu from the distribution package (`/usr/bin/umu-run`, system Python).
pub fn umu_system(fs: &MemFs) {
    exe(fs, "/usr/bin/python3", "\x7fELF");
    exe(
        fs,
        "/usr/bin/umu-run",
        "#!/usr/bin/python3\nfrom umu.__main__ import main\n",
    );
}

/// umu installed with `pipx` into a venv on the system Python.
pub fn umu_pipx(fs: &MemFs, home: &str) {
    exe(fs, "/usr/bin/python3.13", "\x7fELF");
    let venv = format!("{home}/.local/share/pipx/venvs/umu-launcher");
    fs.file(
        format!("{venv}/pyvenv.cfg"),
        "home = /usr/bin\ninclude-system-site-packages = false\n",
    )
    .dir(format!("{venv}/lib/python3.13/site-packages/umu"))
    .link(format!("{venv}/bin/python"), "/usr/bin/python3.13");
    exe(
        fs,
        &format!("{venv}/bin/umu-run"),
        &format!("#!{venv}/bin/python\nimport umu\n"),
    );
    fs.link(
        format!("{home}/.local/bin/umu-run"),
        format!("{venv}/bin/umu-run"),
    );
}

/// umu installed with `uv tool install` (venv on a uv-managed CPython under the home).
pub fn umu_uv(fs: &MemFs, home: &str) {
    let py = format!("{home}/.local/share/uv/python/cpython-3.13.1-linux-x86_64-gnu");
    exe(fs, &format!("{py}/bin/python3.13"), "\x7fELF");
    fs.dir(format!("{py}/lib/python3.13"));
    let tool = format!("{home}/.local/share/uv/tools/umu-launcher");
    fs.file(
        format!("{tool}/pyvenv.cfg"),
        format!("home = {py}/bin\nuv = 0.9.0\n"),
    )
    .dir(format!("{tool}/lib/python3.13/site-packages/umu"))
    .link(format!("{tool}/bin/python"), format!("{py}/bin/python3.13"));
    exe(
        fs,
        &format!("{tool}/bin/umu-run"),
        &format!("#!{tool}/bin/python\nimport umu\n"),
    );
    fs.link(
        format!("{home}/.local/bin/umu-run"),
        format!("{tool}/bin/umu-run"),
    );
}

/// nixpkgs' umu: a `makeWrapper` script handing over to a bwrap-based FHS wrapper.
pub fn umu_nix_wrapper(fs: &MemFs) {
    exe(fs, "/nix/store/dddd-bash-5.3/bin/bash", "\x7fELF");
    let fhs = "/nix/store/hhhh-umu-launcher-1.4.4-bwrap";
    exe(
        fs,
        fhs,
        "#!/nix/store/dddd-bash-5.3/bin/bash\nignored=(/nix /dev /proc /etc /tmp)\nfor i in /nix/store/llll-umu-launcher-1.4.4-fhsenv-rootfs/*; do :; done\ncmd=(\n  /nix/store/iiii-bubblewrap-0.12.0/bin/bwrap\n  --dev-bind /dev /dev\n)\nexec \"${cmd[@]}\"\n",
    );
    let wrapper = "/nix/store/jjjj-umu-launcher-1.4.4/bin/umu-run";
    exe(
        fs,
        wrapper,
        &format!(
            "#!/nix/store/dddd-bash-5.3/bin/bash\nexport PATH=/nix/store/kkkk/bin\nexec -a \"$0\" \"{fhs}\" \"$@\"\n"
        ),
    );
    fs.link("/run/current-system/sw/bin/umu-run", wrapper);
}
