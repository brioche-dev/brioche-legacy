// import { sh } from "@brioche-dev/v0";

function sh(template, ...args) {
    if (template.length > 1 || args.length > 0) {
        throw new Error("Cannot interpolate values");
    }

    return {
        shell: "sh",
        script: template[0],
        envVars: {},
    };
}

const VERSION = "2.37";

// Based on Linux From Scratch v11.0 Chapter 5.2
// https://www.linuxfromscratch.org/lfs/view/11.0/chapter05/binutils-pass1.html
export const recipe = {
    options: {},
    definition: () => ({
        name: "brioche-bootstrap-phase1-binutils",
        version: VERSION,
        source: {
            tarball: `https://ftp.gnu.org/gnu/binutils/binutils-${VERSION}.tar.gz`,
        },
        dependencies: {},
        build: sh`
            apk add build-base
            cd ./binutils-*
            env
            sleep 10
            mkdir -v build
            cd build
            ../configure \
                --prefix="$BRIOCHE_PREFIX" \
                --with-sysroot="$BRIOCHE_PREFIX" \
                --target="$BRIOCHE_BOOTSTRAP_TARGET" \
                --disable-nls \
                --disable-werror
            make -j1
            make install
        `,
    }),
};
