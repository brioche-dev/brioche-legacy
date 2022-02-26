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

const VERSION = "5.13.12";

// Based on Linux From Scratch v11.0 Chapter 5.4
// https://www.linuxfromscratch.org/lfs/view/stable/chapter05/linux-headers.html
export const recipe = {
    options: {},
    definition: () => ({
        name: "brioche-bootstrap-phase1-linux-headers",
        version: VERSION,
        source: {
            tarball: `https://www.kernel.org/pub/linux/kernel/v5.x/linux-5.13.12.tar.gz`,
        },
        dependencies: {},
        build: sh`
            set -eu

            export PATH="$BRIOCHE_PREFIX/tools/bin\${PATH:+:$PATH}"
            apk add build-base
            cd linux-*

            make mrproper
            make headers
            find usr/include -name '.*' -delete
            rm usr/include/Makefile

            mkdir "$BRIOCHE_PREFIX/usr"
            cp -rv usr/include "$BRIOCHE_PREFIX/usr"
        `,
    }),
};
