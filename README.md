# Brioche

Brioche is a package manager and build tool (or it will be one day!)

## Project goals

The end goal of Brioche is to create a package manager/build tool that combines the best elements of existing tools with some fairly straightforward improvements. Some major influences for Brioche are Cargo, Homebrew, and Nix. Some future goals:

- Easy to set up and use
- Build and install packages for a local user without requiring root privileges
- Deterministic (or at least consistent) builds
- Easy to publish new packages and contribute updates for existing packages
- Set up and build local projects (no more "works on my machine" issues while onboarding)
- Easy cross-compilation of packages

## Current status

Brioche is still in the "pre-proof-of-concept" stage, and isn't yet ready for prime-time. Here is a basic feature list that needs to be done before it can graduate to the "proof-of-concept" stage:

- [ ] Configuration format for defining packages (the current plan is to use TypeScript)
- [ ] Some form of sandboxing or isolation for building packages
- [ ] Store packages based on package configuration hash, so package builds can be trivially cached
- [ ] Command-line tools for managing packages
- [ ] Infrastructure for distributing pre-built packages (i.e. a repo with some sort of build-bot)
- [ ] An initial repo of common packages (so you actually have stuff to use with Brioche)
