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

// Based on Linux From Scratch v11.0 Chapter 5.3
// https://www.linuxfromscratch.org/lfs/view/11.0/chapter05/gcc-pass1.html
export const recipe = {
    options: {},
    definition: () => ({
        name: "brioche-bootstrap-phase1-gcc",
        version: VERSION,
        source: {
            tarball: `https://ftp.gnu.org/gnu/gcc/gcc-11.2.0/gcc-11.2.0.tar.gz`,
        },
        dependencies: {
            "brioche-bootstrap-phase1-binutils": "2.37",
        },
        build: sh`
            set -eu

            apk add build-base
            wget https://www.mpfr.org/mpfr-4.1.0/mpfr-4.1.0.tar.xz
            wget https://ftp.gnu.org/gnu/gmp/gmp-6.2.1.tar.xz
            wget https://ftp.gnu.org/gnu/mpc/mpc-1.2.1.tar.gz

            cd gcc-*
            tar -xf ../mpfr-*.tar.xz
            mv -v mpfr-* mpfr
            tar -xf ../gmp-*.tar.xz
            mv -v gmp-* gmp
            tar -xf ../mpc-*.tar.gz
            mv -v mpc-* mpc

            case $(uname -m) in
                x86_64)
                    sed -e '/m64=/s/lib64/lib/' \
                        -i.orig \
                        gcc/config/i386/t-linux64
                ;;
            esac

            mkdir -v build
            cd build
            ../configure \
                --target="$BRIOCHE_BOOTSTRAP_TARGET" \
                --prefix="$BRIOCHE_PREFIX" \
                --with-glibc-version=2.11 \
                --with-sysroot="$BRIOCHE_PREFIX" \
                --with-newlib \
                --without-headers \
                --enable-initfini-array \
                --disable-nls \
                --disable-shared \
                --disable-multilib \
                --disable-decimal-float \
                --disable-threads \
                --disable-libatomic \
                --disable-libgomp \
                --disable-libquadmath \
                --disable-libssp \
                --disable-libvtv \
                --disable-libstdcxx \
                --enable-languages=c,c++
            make
            make install

            cd ..
            libgcc_filename="$("\${BRIOCHE_BOOTSTRAP_TARGET}-gcc" -print-libgcc-file-name)"
            libgcc_limits_h="$(dirname "$libgcc_filename")/install-tools/include/limits.h"
            cat gcc/limitx.h gcc/glimits.h gcc/limity.h > "$libgcc_limits_h"
        `,
    }),
};
