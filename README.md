NOTE: I am building this using kiro, using a spec based approach.
Intial thoughs and ideas: https://blog.tovganesh.in/2012/01/kosh-building-mobile-user-experience.html
At this point this is my personal toy project. I currently have no idea if this will actually work. If you are interested in writing a rust based OS with or without AI tools you can poke me.


# Kosh Operating System

A modern, microkernel-based operating system written in Rust, designed with security, modularity, and cross-platform support in mind.

## Features

- **Microkernel Architecture**: Minimal kernel with services running in userspace
- **Memory Safety**: Written in Rust for memory safety and performance
- **Cross-Platform**: Supports x86-64 and ARM64 architectures
- **Modular Design**: Pluggable driver system and userspace services
- **Capability-Based Security**: Fine-grained access control system

## Architecture

### Core Components

- **Kernel**: Minimal microkernel handling basic system operations
- **Drivers**: Modular drivers for storage, network, and graphics
- **Userspace Services**: 
  - Init process for system startup
  - Filesystem service
  - Driver manager for dynamic driver loading

### Shared Libraries

- **kosh-types**: Common type definitions and interfaces
- **kosh-ipc**: Inter-process communication primitives

## Building

### Prerequisites

- Rust nightly toolchain (automatically configured via `rust-toolchain.toml`)
- GRUB tools (optional, for ISO creation)

### Quick Start

```bash
# Build all components
./scripts/build.sh

# Create bootable ISO
./scripts/build-iso.sh
```

### Manual Building

```bash
# Build kernel for x86-64
cargo build --package kosh-kernel --target x86_64-kosh.json --release -Z build-std=core,alloc

# Build userspace components
cargo build --package kosh-init --target x86_64-kosh.json --release -Z build-std=core,alloc
cargo build --package kosh-fs-service --target x86_64-kosh.json --release -Z build-std=core,alloc
cargo build --package kosh-driver-manager --target x86_64-kosh.json --release -Z build-std=core,alloc

# Build drivers (standard target)
cargo build --package kosh-storage-driver --release
cargo build --package kosh-network-driver --release
cargo build --package kosh-graphics-driver --release
```

## Project Structure

```
kosh/
├── kernel/                 # Microkernel implementation
├── drivers/               # Hardware drivers
│   ├── storage/          # Storage device drivers
│   ├── network/          # Network device drivers
│   └── graphics/         # Graphics device drivers
├── userspace/            # Userspace services
│   ├── init/            # System initialization
│   ├── fs-service/      # Filesystem service
│   └── driver-manager/  # Driver management service
├── shared/              # Shared libraries
│   ├── kosh-types/     # Common type definitions
│   └── kosh-ipc/       # IPC primitives
├── scripts/            # Build and utility scripts
├── iso/               # ISO image structure (generated)
└── build/             # Build artifacts (generated)
```

## Supported Platforms

- **x86-64**: Primary development target
- **ARM64**: Planned support (target specification ready)

## Development

### Target Specifications

- `x86_64-kosh.json`: Custom target for x86-64 bare-metal
- `aarch64-kosh.json`: Custom target for ARM64 bare-metal

### Build Configuration

The project uses Rust nightly with custom target specifications for bare-metal development. The build system automatically handles:

- Cross-compilation for multiple architectures
- No-std environment setup
- Custom bootloader integration
- ISO image generation

## Testing

```bash
# Run tests for workspace components
cargo test --workspace

# Test kernel specifically
cargo test --package kosh-kernel --target x86_64-kosh.json -Z build-std=core,alloc
```

## Running

### QEMU

```bash
# Run with QEMU (x86-64)
qemu-system-x86_64 -cdrom kosh.iso

# Run with QEMU (ARM64)
qemu-system-aarch64 -M virt -cpu cortex-a57 -cdrom kosh.iso
```

### Real Hardware

Flash the generated ISO to a USB drive or burn to CD/DVD for real hardware testing.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and ensure builds pass
5. Submit a pull request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Roadmap

- [x] Basic project structure and build system
- [ ] Memory management implementation
- [ ] Process management and scheduling
- [ ] IPC mechanism implementation
- [ ] Driver framework completion
- [ ] Filesystem implementation
- [ ] Network stack
- [ ] Graphics subsystem
- [ ] ARM64 support completion
- [ ] UEFI boot support

## Architecture Decisions

### Microkernel Design

Kosh follows a microkernel architecture where the kernel provides only essential services:
- Memory management
- Process scheduling
- Basic IPC
- Hardware abstraction

All other services (filesystem, networking, drivers) run in userspace for better isolation and reliability.

### Capability-Based Security

The system implements capability-based security where processes must have explicit capabilities to access resources, providing fine-grained access control.

### Rust Language Choice

Rust was chosen for its memory safety guarantees, zero-cost abstractions, and excellent support for systems programming without sacrificing performance.
