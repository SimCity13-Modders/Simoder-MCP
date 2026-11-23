# Third-party components

Original DevTools code uses the MIT LICENSE. The Simoder source SDK has its own
MIT license and is referenced by revision, without copied sources.

The installed authoring worker statically links Luau (MIT) and consumes
tomlplusplus (MIT) through the Simoder SDK. The developer service package includes
their license texts and the SDK license under `licenses/`. The TypeScript
toolchain carries its installed npm dependency licenses with the package files.
Node.js is an external authoring prerequisite and is not embedded in the runtime DLL.

ModelContextProtocol 2.0.0 and Core use Apache-2.0. Microsoft.Extensions.Hosting
10.0.10 declares MIT. Self-contained .NET/Windows SDK components and transitive
NuGet dependencies retain their own terms and accompanying notices. This is a
direct-dependency inventory, not an exhaustive distribution license inventory.

REA is fetched separately through npx and retains its upstream MIT license.
OpenSC5/OpenSCP importers are independently implemented; imported catalog names,
descriptions and data retain source terms. No external catalog or proprietary
EA binary/package is bundled. Provenance hashes do not grant redistribution rights.
