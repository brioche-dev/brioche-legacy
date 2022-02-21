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

const VERSION = "5.40";

export const recipe = {
    options: {},
    definition: () => ({
        name: "file",
        version: VERSION,
        source: {
            // git: "https://github.com/file/file",
            // ref: `FILE${VERSION.replace(".", "_")}`,
            tarball: `https://astron.com/pub/file/file-${VERSION}.tar.gz`,
        },
        dependencies: {},
        build: sh`
            apk add build-base
            cd ./file-*
            ./configure
            make
            make install
        `,
    }),
};
