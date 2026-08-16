#define WIN32_LEAN_AND_MEAN
#define UNICODE
#define _UNICODE
#include <windows.h>

#define TEXT_CAPACITY 32768u
#define INVALID_INDEX 0xffffffffu
#define LAUNCH_PROTOCOL_VALUE L"3"
#define WORKER_PROTOCOL_VALUE L"2"

static const WCHAR launch_protocol_name[] =
    L"SWAWKIT_PROJ_CORE_LAUNCH_PROTOCOL";
static const WCHAR worker_protocol_name[] =
    L"SWAWKIT_PROJ_CORE_LAUNCH_WORKER_PROTOCOL";

static WCHAR entry_path[TEXT_CAPACITY];
static WCHAR core_path[TEXT_CAPACITY];
static WCHAR selector_path[TEXT_CAPACITY];
static WCHAR bootstrap_path[TEXT_CAPACITY];
static WCHAR powershell_path[TEXT_CAPACITY];
static WCHAR child_command_line[TEXT_CAPACITY];
static WCHAR worker_protocol[16u];
static CHAR release_selector[66u];
static STARTUPINFOW startup_info;
static PROCESS_INFORMATION process_info;
static DWORD layout_home_length;

__declspec(noreturn) void __cdecl __report_rangecheckfailure(void)
{
    ExitProcess(1u);
}

static DWORD wide_length(const WCHAR *value)
{
    DWORD length = 0;
    while (value[length] != L'\0') {
        ++length;
    }
    return length;
}

static DWORD narrow_length(const CHAR *value)
{
    DWORD length = 0;
    while (value[length] != '\0') {
        ++length;
    }
    return length;
}

static void fail(BOOL host_mode, const WCHAR *dialog_text, const CHAR *console_text)
{
    HANDLE error_handle = GetStdHandle(STD_ERROR_HANDLE);
    DWORD written = 0;
    BOOL reported = FALSE;

    if (!host_mode && error_handle != NULL && error_handle != INVALID_HANDLE_VALUE) {
        reported = WriteFile(
            error_handle,
            console_text,
            narrow_length(console_text),
            &written,
            NULL
        );
    }
    if (!reported) {
        MessageBoxW(
            NULL,
            dialog_text,
            L"Swaw Kit Proj Launcher",
            MB_OK | MB_ICONERROR
        );
    }
    ExitProcess(1u);
}

static BOOL environment_variable_exists(const WCHAR *name)
{
    WCHAR value;
    DWORD length;

    SetLastError(ERROR_SUCCESS);
    length = GetEnvironmentVariableW(name, &value, 1u);
    return length > 0u || GetLastError() != ERROR_ENVVAR_NOT_FOUND;
}

static BOOL read_environment_variable(
    const WCHAR *name,
    WCHAR *value,
    DWORD capacity
)
{
    DWORD length = GetEnvironmentVariableW(name, value, capacity);
    return length > 0u && length < capacity;
}

static BOOL wide_equal(const WCHAR *left, const WCHAR *right)
{
    DWORD index = 0u;
    while (left[index] != L'\0' && right[index] != L'\0') {
        if (left[index] != right[index]) {
            return FALSE;
        }
        ++index;
    }
    return left[index] == right[index];
}

static BOOL prepare_startup_info(BOOL inherit_handles)
{
    startup_info.cb = sizeof(startup_info);
    startup_info.dwFlags = 0u;
    startup_info.hStdInput = NULL;
    startup_info.hStdOutput = NULL;
    startup_info.hStdError = NULL;
    if (!inherit_handles) {
        return TRUE;
    }

    startup_info.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup_info.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup_info.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (startup_info.hStdInput == NULL
        || startup_info.hStdInput == INVALID_HANDLE_VALUE
        || startup_info.hStdOutput == NULL
        || startup_info.hStdOutput == INVALID_HANDLE_VALUE
        || startup_info.hStdError == NULL
        || startup_info.hStdError == INVALID_HANDLE_VALUE) {
        return FALSE;
    }
    startup_info.dwFlags = STARTF_USESTDHANDLES;
    return TRUE;
}

static BOOL consume_worker_mode(BOOL host_mode, BOOL *worker_mode)
{
    BOOL has_protocol = environment_variable_exists(worker_protocol_name);

    *worker_mode = FALSE;
    if (!has_protocol) {
        return TRUE;
    }
    if (host_mode
        || !read_environment_variable(
            worker_protocol_name,
            worker_protocol,
            16u
        )
        || !wide_equal(worker_protocol, WORKER_PROTOCOL_VALUE)
        || !SetEnvironmentVariableW(worker_protocol_name, NULL)) {
        return FALSE;
    }
    *worker_mode = TRUE;
    return TRUE;
}

static DWORD last_separator_before(const WCHAR *value, DWORD before)
{
    while (before > 0u) {
        --before;
        if (value[before] == L'\\' || value[before] == L'/') {
            return before;
        }
    }
    return INVALID_INDEX;
}

static BOOL copy_path_with_suffix(
    const WCHAR *source,
    DWORD prefix_length,
    const WCHAR *suffix,
    WCHAR *destination
)
{
    DWORD suffix_length = wide_length(suffix);
    DWORD index;

    if (prefix_length + suffix_length + 1u > TEXT_CAPACITY) {
        return FALSE;
    }
    for (index = 0u; index < prefix_length; ++index) {
        destination[index] = source[index];
    }
    for (index = 0u; index <= suffix_length; ++index) {
        destination[prefix_length + index] = suffix[index];
    }
    return TRUE;
}

static BOOL is_file(const WCHAR *path)
{
    DWORD attributes = GetFileAttributesW(path);
    return attributes != INVALID_FILE_ATTRIBUTES
        && (attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)) == 0u;
}

static BOOL resolve_current_core(void)
{
    static const WCHAR release_prefix[] =
        L"\\_lib\\proj\\_bin\\releases\\";
    static const WCHAR core_suffix[] = L"\\swawkit-proj.exe";
    DWORD attributes = GetFileAttributesW(selector_path);
    HANDLE file;
    DWORD bytes_read = 0u;
    DWORD index;
    DWORD destination = 0u;

    core_path[0] = L'\0';
    if (attributes == INVALID_FILE_ATTRIBUTES
        || (attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)) != 0u) {
        return FALSE;
    }
    file = CreateFileW(
        selector_path,
        GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
        NULL
    );
    if (file == INVALID_HANDLE_VALUE
        || !ReadFile(
            file,
            release_selector,
            sizeof(release_selector),
            &bytes_read,
            NULL
        )) {
        if (file != INVALID_HANDLE_VALUE) {
            CloseHandle(file);
        }
        return FALSE;
    }
    CloseHandle(file);
    if (bytes_read != 65u || release_selector[64u] != '\n') {
        return FALSE;
    }
    for (index = 0u; index < 64u; ++index) {
        CHAR value = release_selector[index];
        if (!((value >= '0' && value <= '9')
            || (value >= 'a' && value <= 'f'))) {
            return FALSE;
        }
    }
    if (layout_home_length
            + wide_length(release_prefix)
            + 64u
            + wide_length(core_suffix)
            + 1u
        > TEXT_CAPACITY) {
        return FALSE;
    }
    for (index = 0u; index < layout_home_length; ++index) {
        core_path[destination++] = entry_path[index];
    }
    for (index = 0u; release_prefix[index] != L'\0'; ++index) {
        core_path[destination++] = release_prefix[index];
    }
    for (index = 0u; index < 64u; ++index) {
        core_path[destination++] = (WCHAR)release_selector[index];
    }
    for (index = 0u; core_suffix[index] != L'\0'; ++index) {
        core_path[destination++] = core_suffix[index];
    }
    core_path[destination] = L'\0';
    return is_file(core_path);
}

static BOOL try_layout(DWORD home_length)
{
    static const WCHAR selector_suffix[] = L"\\_lib\\proj\\_bin\\current";
    static const WCHAR bootstrap_suffix[] = L"\\_lib\\proj\\bootstrap.ps1";

    layout_home_length = home_length;
    return copy_path_with_suffix(
            entry_path,
            home_length,
            selector_suffix,
            selector_path
        )
        && copy_path_with_suffix(
            entry_path,
            home_length,
            bootstrap_suffix,
            bootstrap_path
        )
        && (resolve_current_core() || is_file(bootstrap_path));
}

static BOOL locate_layout(void)
{
    DWORD entry_length = wide_length(entry_path);
    DWORD launcher_directory = last_separator_before(entry_path, entry_length);

    return launcher_directory != INVALID_INDEX
        && try_layout(launcher_directory);
}

static BOOL locate_windows_powershell(void)
{
    static const WCHAR suffix[] =
        L"\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    DWORD length = GetWindowsDirectoryW(powershell_path, TEXT_CAPACITY);

    return length > 0u
        && length < TEXT_CAPACITY
        && copy_path_with_suffix(
            powershell_path,
            length,
            suffix,
            powershell_path
        )
        && is_file(powershell_path);
}

static BOOL build_bootstrap_command_line(void)
{
    static const WCHAR options[] =
        L"\" -NoLogo -NoProfile -NonInteractive "
        L"-ExecutionPolicy Bypass -File \"";
    DWORD powershell_length = wide_length(powershell_path);
    DWORD options_length = wide_length(options);
    DWORD bootstrap_length = wide_length(bootstrap_path);
    DWORD index = 0u;
    DWORD source;

    if (powershell_length + options_length + bootstrap_length + 3u
        > TEXT_CAPACITY) {
        return FALSE;
    }
    child_command_line[index++] = L'\"';
    for (source = 0u; source < powershell_length; ++source) {
        child_command_line[index++] = powershell_path[source];
    }
    for (source = 0u; source < options_length; ++source) {
        child_command_line[index++] = options[source];
    }
    for (source = 0u; source < bootstrap_length; ++source) {
        child_command_line[index++] = bootstrap_path[source];
    }
    child_command_line[index++] = L'\"';
    child_command_line[index] = L'\0';
    return TRUE;
}

static BOOL run_bootstrap(BOOL host_mode, BOOL worker_mode)
{
    DWORD creation_flags = host_mode || worker_mode ? CREATE_NO_WINDOW : 0u;
    BOOL inherit_handles = host_mode ? FALSE : TRUE;
    DWORD wait_result;
    DWORD exit_code;

    if (!is_file(bootstrap_path)
        || !locate_windows_powershell()
        || !build_bootstrap_command_line()) {
        return FALSE;
    }
    if (!prepare_startup_info(inherit_handles)
        || !CreateProcessW(
            powershell_path,
            child_command_line,
            NULL,
            NULL,
            inherit_handles,
            creation_flags,
            NULL,
            NULL,
            &startup_info,
            &process_info
        )) {
        return FALSE;
    }
    CloseHandle(process_info.hThread);
    wait_result = WaitForSingleObject(process_info.hProcess, INFINITE);
    if (wait_result != WAIT_OBJECT_0
        || !GetExitCodeProcess(process_info.hProcess, &exit_code)) {
        CloseHandle(process_info.hProcess);
        return FALSE;
    }
    CloseHandle(process_info.hProcess);
    return exit_code == 0u && resolve_current_core();
}

static const WCHAR *raw_argument_tail(void)
{
    const WCHAR *cursor = GetCommandLineW();
    BOOL quoted = FALSE;

    while (*cursor == L' ' || *cursor == L'\t') {
        ++cursor;
    }
    while (*cursor != L'\0') {
        if (*cursor == L'"') {
            quoted = !quoted;
        } else if (!quoted && (*cursor == L' ' || *cursor == L'\t')) {
            break;
        }
        ++cursor;
    }
    while (*cursor == L' ' || *cursor == L'\t') {
        ++cursor;
    }
    return cursor;
}

static BOOL build_child_command_line(const WCHAR *argument_tail)
{
    DWORD core_length = wide_length(core_path);
    DWORD tail_length = wide_length(argument_tail);
    DWORD index = 0u;
    DWORD source;

    if (core_length + tail_length + 4u > TEXT_CAPACITY) {
        return FALSE;
    }
    child_command_line[index++] = L'"';
    for (source = 0u; source < core_length; ++source) {
        child_command_line[index++] = core_path[source];
    }
    child_command_line[index++] = L'"';
    if (tail_length > 0u) {
        child_command_line[index++] = L' ';
        for (source = 0u; source < tail_length; ++source) {
            child_command_line[index++] = argument_tail[source];
        }
    }
    child_command_line[index] = L'\0';
    return TRUE;
}

static BOOL prepare_environment(BOOL host_mode, BOOL worker_mode)
{
    return SetEnvironmentVariableW(
            launch_protocol_name,
            LAUNCH_PROTOCOL_VALUE
        )
        && SetEnvironmentVariableW(
            L"SWAWKIT_PROJ_CORE_LAUNCH_ENTRY_FILE",
            entry_path
        )
        && SetEnvironmentVariableW(
            L"SWAWKIT_PROJ_CORE_LAUNCH_MODE",
            host_mode
                ? L"internal-host"
                : (worker_mode ? L"worker" : L"cli")
        );
}

void WINAPI launcher_entry(void)
{
    const WCHAR *argument_tail = raw_argument_tail();
    BOOL host_mode = *argument_tail == L'\0';
    BOOL worker_mode = FALSE;
    DWORD entry_length = GetModuleFileNameW(NULL, entry_path, TEXT_CAPACITY);
    DWORD creation_flags;
    BOOL inherit_handles;
    DWORD wait_result;
    DWORD exit_code;

    if (environment_variable_exists(
            L"SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL"
        )) {
        fail(
            FALSE,
            L"Cannot start a Swaw Kit Entry from inside another Entry command.",
            "[ERROR] Cannot start a Swaw Kit Entry from inside another Entry command.\r\n"
        );
    }
    if (!consume_worker_mode(host_mode, &worker_mode)) {
        fail(
            host_mode,
            L"Cannot consume the Web worker launch declaration.",
            "[ERROR] Cannot consume the Web worker launch declaration.\r\n"
        );
    }

    if (entry_length == 0u || entry_length >= TEXT_CAPACITY - 1u) {
        fail(
            host_mode,
            L"Cannot read the Launcher executable path.",
            "[ERROR] Cannot read the Launcher executable path.\r\n"
        );
    }
    if (!locate_layout()) {
        fail(
            host_mode,
            L"Cannot locate the shared Core or Bootstrap entry. "
            L"Keep the Launcher directly in SWAWKIT_HOME.",
            "[ERROR] Cannot locate the shared Core or Bootstrap entry. "
            "Keep the Launcher directly in SWAWKIT_HOME.\r\n"
        );
    }
    if (!is_file(core_path) && !run_bootstrap(host_mode, worker_mode)) {
        fail(
            host_mode,
            L"Bootstrap could not build the shared Swaw Kit Proj executable.",
            "[ERROR] Bootstrap could not build the shared Swaw Kit Proj executable.\r\n"
        );
    }
    if (!build_child_command_line(argument_tail)) {
        fail(
            host_mode,
            L"The Launcher command line is too long.",
            "[ERROR] The Launcher command line is too long.\r\n"
        );
    }
    if (!prepare_environment(host_mode, worker_mode)) {
        fail(
            host_mode,
            L"Cannot prepare the shared Proj process environment.",
            "[ERROR] Cannot prepare the shared Proj process environment.\r\n"
        );
    }

    if (host_mode) {
        FreeConsole();
    }
    creation_flags = host_mode || worker_mode ? CREATE_NO_WINDOW : 0u;
    inherit_handles = host_mode ? FALSE : TRUE;
    if (!prepare_startup_info(inherit_handles)
        || !CreateProcessW(
            core_path,
            child_command_line,
            NULL,
            NULL,
            inherit_handles,
            creation_flags,
            NULL,
            NULL,
            &startup_info,
            &process_info
        )) {
        fail(
            host_mode,
            L"Cannot start the shared Swaw Kit Proj executable.",
            "[ERROR] Cannot start the shared Swaw Kit Proj executable.\r\n"
        );
    }

    CloseHandle(process_info.hThread);
    if (host_mode) {
        CloseHandle(process_info.hProcess);
        ExitProcess(0u);
    }

    wait_result = WaitForSingleObject(process_info.hProcess, INFINITE);
    if (wait_result != WAIT_OBJECT_0
        || !GetExitCodeProcess(process_info.hProcess, &exit_code)) {
        CloseHandle(process_info.hProcess);
        fail(
            FALSE,
            L"Cannot read the shared Proj process result.",
            "[ERROR] Cannot read the shared Proj process result.\r\n"
        );
    }
    CloseHandle(process_info.hProcess);
    ExitProcess(exit_code);
}
