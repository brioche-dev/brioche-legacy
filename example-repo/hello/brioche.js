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

const VERSION = "2.12";

export const recipe = {
    options: {},
    definition: () => ({
        name: "hello",
        version: VERSION,
        source: {
            git: "https://git.savannah.gnu.org/git/hello.git",
            ref: `v${VERSION}`,
        },
        dependencies: {},
        build: sh`
            ./bootstrap
            make
            make install
        `,
    }),
};
