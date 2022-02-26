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

const VERSION = "2.34";

// Based on Linux From Scratch v11.0 Chapter 5.5
// https://www.linuxfromscratch.org/lfs/view/stable/chapter05/glibc.html
export const recipe = {
    options: {},
    definition: () => ({
        name: "brioche-bootstrap-phase1-glibc",
        version: VERSION,
        source: {
            tarball: `https://ftp.gnu.org/gnu/glibc/glibc-2.34.tar.gz`,
        },
        dependencies: {
            "brioche-bootstrap-phase1-binutils": "2.37",
            "brioche-bootstrap-phase1-gcc": "2.37",
            "brioche-bootstrap-phase1-linux-headers": "5.13.12",
        },
        build: sh`
            set -eu

            export PATH="$BRIOCHE_PREFIX/tools/bin\${PATH:+:$PATH}"
            apk add build-base gawk bison python3 grep
            wget 'https://www.linuxfromscratch.org/patches/lfs/11.0/glibc-2.34-fhs-1.patch'

            cd glibc-*/

            case $(uname -m) in
                i?86)
                    ln -sfv ld-linux.so.2 "$BRIOCHE_PREFIX/lib/ld-lsb.so.3"
                    ;;
                x86_64)
                    mkdir -p "$BRIOCHE_PREFIX/lib64"
                    ln -sfv ../lib/ld-linux-x86-64.so.2 "$BRIOCHE_PREFIX/lib64"
                    ln -sfv ../lib/ld-linux-x86-64.so.2 "$BRIOCHE_PREFIX/lib64/ld-lsb-x86-64.so.3"
                    ;;
            esac
            patch -Np1 -i ../glibc-2.34-fhs-1.patch

            mkdir -v build
            cd build
            echo "rootsbindir=/usr/sbin" > configparms

            ../configure \
                --prefix=/usr \
                --host="$BRIOCHE_BOOTSTRAP_TARGET" \
                --build="$(../scripts/config.guess)" \
                --enable-kernel=3.2 \
                --with-headers="$BRIOCHE_PREFIX/usr/include" \
                libc_cv_slibdir=/usr/lib

            make
            make DESTDIR="$BRIOCHE_PREFIX" install

            sed '/RTLDLIST=/s@/usr@@g' -i "$BRIOCHE_PREFIX/usr/bin/ldd"

            "$BRIOCHE_PREFIX/tools/libexec/gcc/$BRIOCHE_BOOTSTRAP_TARGET/11.2.0/install-tools/mkheaders"
        `,
    }),
};
