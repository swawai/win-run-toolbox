#define WIN32_LEAN_AND_MEAN
#define UNICODE
#define _UNICODE
#include <windows.h>

#define TEXT_CAPACITY 32768u
#define INVALID_INDEX 0xffffffffu

static WCHAR entry_path[TEXT_CAPACITY];
static WCHAR core_path[TEXT_CAPACITY];
static WCHAR bootstrap_path[TEXT_CAPACITY];
static WCHAR powershell_path[TEXT_CAPACITY];
static WCHAR child_command_line[TEXT_CAPACITY];
static STARTUPINFOW startup_info;
static PROCESS_INFORMATION process_info;

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
        && (attributes & FILE_ATTRIBUTE_DIRECTORY) == 0u;
}

static BOOL try_layout(DWORD home_length)
{
    static const WCHAR core_suffix[] = L"\\_lib\\proj\\_bin\\swawkit-proj.exe";
    static const WCHAR bootstrap_suffix[] =
        L"\\_lib\\proj\\_bootstrap\\run.ps1";

    return copy_path_with_suffix(
            entry_path,
            home_length,
            core_suffix,
            core_path
        )
        && copy_path_with_suffix(
            entry_path,
            home_length,
            bootstrap_suffix,
            bootstrap_path
        )
        && (is_file(core_path) || is_file(bootstrap_path));
}

static BOOL locate_layout(void)
{
    DWORD entry_length = wide_length(entry_path);
    DWORD launcher_directory = last_separator_before(entry_path, entry_length);
    DWORD home_directory;

    if (launcher_directory == INVALID_INDEX) {
        return FALSE;
    }
    if (try_layout(launcher_directory)) {
        return TRUE;
    }

    home_directory = last_separator_before(entry_path, launcher_directory);
    return home_directory != INVALID_INDEX && try_layout(home_directory);
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

static BOOL run_bootstrap(BOOL host_mode)
{
    DWORD creation_flags = host_mode ? CREATE_NO_WINDOW : 0u;
    BOOL inherit_handles = host_mode ? FALSE : TRUE;
    DWORD wait_result;
    DWORD exit_code;

    if (!is_file(bootstrap_path)
        || !locate_windows_powershell()
        || !build_bootstrap_command_line()) {
        return FALSE;
    }
    startup_info.cb = sizeof(startup_info);
    if (!CreateProcessW(
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
    return exit_code == 0u && is_file(core_path);
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

static BOOL prepare_environment(BOOL host_mode)
{
    static const WCHAR *clear_names[] = {
        L"SWAWKIT_HOME",
        L"SWAWKIT_PROJ_PROTOCOL",
        L"SWAWKIT_PROJ_TARGET_PROJECT_ROOT",
        L"SWAWKIT_PROJ_ACTION_ROOT",
        L"SWAWKIT_PROJ_DATA_ROOT",
        L"SWAWKIT_PROJ_ENTRY_COMMAND",
        L"SWAWKIT_PROJ_COMMAND_PROTOCOL",
        L"SWAWKIT_PROJ_COMMAND_PHASE",
        L"SWAWKIT_PROJ_COMMAND_ADDRESS",
        L"SWAWKIT_PROJ_COMMAND_DIR",
        L"SWAWKIT_PROJ_GUARD_SCOPE",
        L"SWAWKIT_PROJ_HELP_TARGET_ADDRESS",
        L"SWAWKIT_PROJ_INVOCATION_DIR",
        L"SWAWKIT_PROJ_INTERNAL_RUNTIME_WORKING_DIR"
    };
    DWORD index;

    for (index = 0u; index < sizeof(clear_names) / sizeof(clear_names[0]); ++index) {
        if (!SetEnvironmentVariableW(clear_names[index], NULL)) {
            return FALSE;
        }
    }
    return SetEnvironmentVariableW(L"SWAWKIT_PROJ_ENTRY_FILE", entry_path)
        && SetEnvironmentVariableW(
            L"SWAWKIT_PROJ_LAUNCH_MODE",
            host_mode ? L"internal-host" : L"cli"
        );
}

void WINAPI launcher_entry(void)
{
    const WCHAR *argument_tail = raw_argument_tail();
    BOOL host_mode = *argument_tail == L'\0';
    DWORD entry_length = GetModuleFileNameW(NULL, entry_path, TEXT_CAPACITY);
    DWORD creation_flags = host_mode ? CREATE_NO_WINDOW : 0u;
    BOOL inherit_handles = host_mode ? FALSE : TRUE;
    DWORD wait_result;
    DWORD exit_code;

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
            L"Keep the Launcher in SWAWKIT_HOME or one of its direct child directories.",
            "[ERROR] Cannot locate the shared Core or Bootstrap entry. "
            "Keep the Launcher in SWAWKIT_HOME or one of its direct child directories.\r\n"
        );
    }
    if (!is_file(core_path) && !run_bootstrap(host_mode)) {
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
    if (!prepare_environment(host_mode)) {
        fail(
            host_mode,
            L"Cannot prepare the shared Proj process environment.",
            "[ERROR] Cannot prepare the shared Proj process environment.\r\n"
        );
    }

    if (host_mode) {
        FreeConsole();
    }
    startup_info.cb = sizeof(startup_info);
    if (!CreateProcessW(
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
