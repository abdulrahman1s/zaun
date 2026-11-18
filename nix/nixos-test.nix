# VM test for nixosModules.default: the module evaluates on a minimal system, the wrapped
# binary runs, the static shim is where the binary looks for it, bubblewrap works for a
# normal user, and the MIME integration only adds associations.
{ self, pkgs }:

let
  zaun = self.packages.${pkgs.stdenv.hostPlatform.system}.zaun;
  bwrap = "${pkgs.bubblewrap}/bin/bwrap";
in
pkgs.testers.runNixOSTest {
  name = "zaun-nixos-module";

  nodes.machine = {
    imports = [ self.nixosModules.default ];

    programs.zaun.enable = true;
    programs.zaun.backgroundService.enable = true;

    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
    };

    environment.systemPackages = [ pkgs.file ];
    virtualisation.memorySize = 1024;
  };

  testScript = ''
    import re

    machine.wait_for_unit("multi-user.target")

    def user(cmd):
        return machine.succeed(f"su - alice -c '{cmd}'")

    with subtest("zaun --version runs as a normal user"):
        out = user("zaun --version")
        assert "${zaun.version}" in out, out

    with subtest("the init shim is installed in libexec and statically linked"):
        shim = "${zaun}/libexec/zaun/zaun-init"
        machine.succeed(f"test -x {shim}")
        info = machine.succeed(f"file -L {shim}")
        assert "statically linked" in info or "static-pie linked" in info, info

    with subtest("the Chat helper and its engine library are installed in libexec"):
        machine.succeed("test -x ${zaun}/libexec/zaun/zaun-chat")
        machine.succeed("test -f ${zaun}/libexec/zaun/simplex/libsimplex.so")

    with subtest("bubblewrap creates user namespaces as a normal user, also nested"):
        user("${bwrap} --unshare-user --ro-bind / / true")
        # The outer sandbox needs its own /proc so the inner bwrap can write its uid_map
        # (pressure-vessel nests the same way inside Zaun's sandbox, TECHNICAL_RESEARCH F-401).
        user("${bwrap} --unshare-user --unshare-pid --ro-bind / / --dev /dev --proc /proc ${bwrap} --unshare-user --ro-bind / / true")

    with subtest("umu-run and bwrap are on the wrapped PATH"):
        wrapper = machine.succeed("cat $(readlink -f $(su - alice -c 'command -v zaun'))")
        dirs = re.findall(r"/nix/store/[^:'\"\s]+/bin", wrapper)
        machine.succeed(" || ".join(f"test -x {d}/umu-run" for d in dirs))
        machine.succeed(" || ".join(f"test -x {d}/bwrap" for d in dirs))

    with subtest("desktop entry installed; MIME associations added, defaults untouched"):
        machine.succeed("test -f /run/current-system/sw/share/applications/io.github.zaun.Zaun.desktop")
        mimeapps = machine.succeed("cat /etc/xdg/mimeapps.list")
        assert "[Added Associations]" in mimeapps, mimeapps
        assert "application/x-ms-dos-executable=io.github.zaun.Zaun.desktop" in mimeapps, mimeapps
        assert "[Default Applications]" not in mimeapps, mimeapps

    with subtest("the background service is a user unit that leaves games running"):
        unit = machine.succeed("cat /etc/systemd/user/zaun.service")
        assert "zaun service" in unit, unit
        assert "KillMode=process" in unit, unit
        machine.succeed("test -e /etc/systemd/user/graphical-session.target.wants/zaun.service")

    with subtest("32-bit graphics drivers are enabled"):
        machine.succeed("test -e /run/opengl-driver-32")
  '';
}
