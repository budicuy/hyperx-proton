/*
 * Minimal native Windows launcher shim (steam.exe) for HyperX-Proton.
 *
 * Some game anti-cheat modules (e.g. HoYoverse HoYoProtect / MHYPBase) check
 * that the game process was spawned by steam.exe on Linux/Proton. This tiny
 * standalone shim performs no Steam client IPC and simply spawns the target
 * executable with inherited handles/working directory, waits for exit, and
 * forwards its exit code.
 */
#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdlib.h>
#include <wchar.h>

typedef struct {
    wchar_t *data;
    size_t len;
    size_t cap;
} WBuf;

static int wbuf_reserve(WBuf *b, size_t extra) {
    if (b->len + extra + 1 <= b->cap) return 1;
    size_t ncap = b->cap ? b->cap * 2 : 256;
    while (ncap < b->len + extra + 1) ncap *= 2;
    wchar_t *nd = (wchar_t *)realloc(b->data, ncap * sizeof(wchar_t));
    if (!nd) return 0;
    b->data = nd;
    b->cap = ncap;
    return 1;
}

static int wbuf_push_ch(WBuf *b, wchar_t c) {
    if (!wbuf_reserve(b, 1)) return 0;
    b->data[b->len++] = c;
    b->data[b->len] = L'\0';
    return 1;
}

static int wbuf_append_quoted(WBuf *b, const wchar_t *arg) {
    int needs_quotes = (*arg == L'\0');
    for (const wchar_t *p = arg; *p; ++p) {
        if (*p == L' ' || *p == L'\t' || *p == L'"') {
            needs_quotes = 1;
            break;
        }
    }

    if (!needs_quotes) {
        for (const wchar_t *p = arg; *p; ++p) {
            if (!wbuf_push_ch(b, *p)) return 0;
        }
        return 1;
    }

    if (!wbuf_push_ch(b, L'"')) return 0;
    size_t backslashes = 0;
    for (const wchar_t *p = arg; *p; ++p) {
        if (*p == L'\\') {
            ++backslashes;
        } else if (*p == L'"') {
            for (size_t i = 0; i < backslashes * 2 + 1; ++i) {
                if (!wbuf_push_ch(b, L'\\')) return 0;
            }
            if (!wbuf_push_ch(b, L'"')) return 0;
            backslashes = 0;
        } else {
            for (size_t i = 0; i < backslashes; ++i) {
                if (!wbuf_push_ch(b, L'\\')) return 0;
            }
            backslashes = 0;
            if (!wbuf_push_ch(b, *p)) return 0;
        }
    }
    for (size_t i = 0; i < backslashes * 2; ++i) {
        if (!wbuf_push_ch(b, L'\\')) return 0;
    }
    return wbuf_push_ch(b, L'"');
}

static wchar_t *convert_unix_path_if_needed(const wchar_t *arg) {
    if (arg[0] != L'/') return NULL;
    size_t len = wcslen(arg);
    wchar_t *out = (wchar_t *)malloc((len + 3) * sizeof(wchar_t));
    if (!out) return NULL;
    out[0] = L'Z';
    out[1] = L':';
    for (size_t i = 0; i <= len; ++i) {
        out[i + 2] = (arg[i] == L'/') ? L'\\' : arg[i];
    }
    return out;
}

int wmain(int argc, wchar_t **argv) {
    if (argc < 2) {
        return 2;
    }

    WBuf cmd = {0};
    for (int i = 1; i < argc; ++i) {
        if (i > 1 && !wbuf_push_ch(&cmd, L' ')) {
            free(cmd.data);
            return 1;
        }
        wchar_t *converted = (i == 1) ? convert_unix_path_if_needed(argv[i]) : NULL;
        const wchar_t *piece = converted ? converted : argv[i];
        int ok = wbuf_append_quoted(&cmd, piece);
        free(converted);
        if (!ok) {
            free(cmd.data);
            return 1;
        }
    }

    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    ZeroMemory(&si, sizeof(si));
    ZeroMemory(&pi, sizeof(pi));
    si.cb = sizeof(si);

    if (!CreateProcessW(NULL, cmd.data, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi)) {
        DWORD err = GetLastError();
        free(cmd.data);
        return (int)(err ? err : 1);
    }

    free(cmd.data);
    WaitForSingleObject(pi.hProcess, INFINITE);

    DWORD exit_code = 1;
    GetExitCodeProcess(pi.hProcess, &exit_code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return (int)exit_code;
}
