import { dlopen, FFIType, ptr } from "bun:ffi";

const kernel = dlopen("kernel32.dll", {
  CloseHandle: { args: [FFIType.u64], returns: FFIType.bool },
  CreateFileW: {
    args: [
      FFIType.ptr,
      FFIType.u32,
      FFIType.u32,
      FFIType.ptr,
      FFIType.u32,
      FFIType.u32,
      FFIType.ptr,
    ],
    returns: FFIType.u64,
  },
  GetLastError: { returns: FFIType.u32 },
  GetFileInformationByHandleEx: {
    args: [FFIType.u64, FFIType.u32, FFIType.ptr, FFIType.u32],
    returns: FFIType.bool,
  },
  MoveFileExW: {
    args: [FFIType.ptr, FFIType.ptr, FFIType.u32],
    returns: FFIType.bool,
  },
});

const GENERIC_READ_WRITE = 0xc0000000;
const OPEN_ALWAYS = 4;
const FILE_ATTRIBUTE_NORMAL = 0x80;
const FILE_ATTRIBUTE_DIRECTORY = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
const FILE_BASIC_INFO = 0;
const ERROR_SHARING_VIOLATION = 32;
const ERROR_LOCK_VIOLATION = 33;
const MOVEFILE_REPLACE_EXISTING = 0x1;
const MOVEFILE_WRITE_THROUGH = 0x8;
const INVALID_HANDLE_VALUE = 0xffffffffffffffffn;

function wide(value: string): Buffer {
  return Buffer.from(`${value}\0`, "utf16le");
}

export async function acquireExclusiveFileLock(
  path: string,
  timeoutMilliseconds: number,
): Promise<Disposable> {
  const started = Date.now();
  const encoded = wide(path);
  while (true) {
    const handle = kernel.symbols.CreateFileW(
      ptr(encoded),
      GENERIC_READ_WRITE,
      0,
      null,
      OPEN_ALWAYS,
      FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
      null,
    );
    if (handle !== INVALID_HANDLE_VALUE) {
      assertRegularFileHandle(handle, path);
      let closed = false;
      return {
        [Symbol.dispose]() {
          if (!closed) {
            closed = true;
            if (!kernel.symbols.CloseHandle(handle)) {
              throw new Error(`cannot close build lock '${path}'`);
            }
          }
        },
      };
    }
    const code = kernel.symbols.GetLastError();
    if (
      (code !== ERROR_SHARING_VIOLATION && code !== ERROR_LOCK_VIOLATION)
      || Date.now() - started >= timeoutMilliseconds
    ) {
      throw new Error(
        `cannot acquire build lock '${path}': Win32 error ${code}`,
      );
    }
    await Bun.sleep(200);
  }
}

function assertRegularFileHandle(handle: bigint, path: string): void {
  const information = Buffer.alloc(40);
  if (
    !kernel.symbols.GetFileInformationByHandleEx(
      handle,
      FILE_BASIC_INFO,
      ptr(information),
      information.byteLength,
    )
  ) {
    const code = kernel.symbols.GetLastError();
    kernel.symbols.CloseHandle(handle);
    throw new Error(`cannot inspect build lock '${path}': Win32 error ${code}`);
  }
  const attributes = information.readUInt32LE(32);
  if (
    (attributes & FILE_ATTRIBUTE_DIRECTORY) !== 0
    || (attributes & FILE_ATTRIBUTE_REPARSE_POINT) !== 0
  ) {
    kernel.symbols.CloseHandle(handle);
    throw new Error(`build lock must be a regular non-reparse file: ${path}`);
  }
}

export function moveFileReplace(source: string, destination: string): void {
  const sourceWide = wide(source);
  const destinationWide = wide(destination);
  if (
    !kernel.symbols.MoveFileExW(
      ptr(sourceWide),
      ptr(destinationWide),
      MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
    )
  ) {
    throw new Error(
      `cannot atomically publish '${destination}': Win32 error ${kernel.symbols.GetLastError()}; recovery temporary: '${source}'`,
    );
  }
}
