"""Read bytes at an EE virtual address from the executable's PT_LOAD segments."""
import struct


def read(elf, address, size):
    phoff = struct.unpack_from('<I', elf, 0x1c)[0]
    phnum = struct.unpack_from('<H', elf, 0x2c)[0]
    for i in range(phnum):
        kind, off, va, _pa, filesz, _memsz = struct.unpack_from('<6I', elf, phoff + 32 * i)
        if kind == 1 and va <= address and address + size <= va + filesz:
            return elf[off + address - va: off + address - va + size]
    raise ValueError(f'address {address:#x} not in a loaded file segment')
