#define NOMINMAX
#include <windows.h>
#include <shlobj.h>
#include <wrl/client.h>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>
#include "identity.h"

using Microsoft::WRL::ComPtr;
namespace fs = std::filesystem;

static void check(bool ok, const char *message) {
    if (!ok) throw std::runtime_error(message);
}

template<class Predicate> static void waitFor(Predicate predicate) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(20);
    do {
        if (predicate()) return;
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    } while (std::chrono::steady_clock::now() < deadline);
    throw std::runtime_error("Timed out waiting for the host operation");
}

static std::string read(const fs::path &path) {
    std::ifstream file(path);
    std::string value;
    std::getline(file, value);
    return value;
}

static ComPtr<IContextMenu> query(IClassFactory *factory, const fs::path &path,
                                  const wchar_t *caption) {
    ComPtr<IShellExtInit> init;
    check(SUCCEEDED(factory->CreateInstance(nullptr, IID_PPV_ARGS(&init))), "CreateInstance failed");
    PIDLIST_ABSOLUTE pidl = nullptr;
    check(SUCCEEDED(SHParseDisplayName(path.c_str(), nullptr, &pidl, 0, nullptr)), "Cannot resolve test folder");
    ComPtr<IShellFolder> parent;
    PCUITEMID_CHILD child = nullptr;
    auto result = SHBindToParent(pidl, IID_PPV_ARGS(&parent), &child);
    ComPtr<IDataObject> data;
    if (SUCCEEDED(result)) {
        result = parent->GetUIObjectOf(nullptr, 1, &child, IID_IDataObject, nullptr,
                                      reinterpret_cast<void **>(data.GetAddressOf()));
    }
    CoTaskMemFree(pidl);
    check(SUCCEEDED(result), "Cannot create Explorer selection");
    check(SUCCEEDED(init->Initialize(nullptr, data.Get(), nullptr)), "Initialize failed");
    ComPtr<IContextMenu> menu;
    check(SUCCEEDED(init.As(&menu)), "IContextMenu unavailable");
    auto popup = CreatePopupMenu();
    check(popup != nullptr, "CreatePopupMenu failed");
    result = menu->QueryContextMenu(popup, 0, 1, 100, CMF_NORMAL);
    const auto count = GetMenuItemCount(popup);
    wchar_t text[80]{};
    GetMenuStringW(popup, 0, text, 80, MF_BYPOSITION);
    DestroyMenu(popup);
    check(SUCCEEDED(result), "QueryContextMenu failed");
    if (!caption) {
        check(count == 0, "Unexpected action on an unrelated or recovery folder");
        return nullptr;
    }
    if (count == 0) return nullptr; // Host can still be committing the previous operation.
    check(count == 1 && std::wstring(text) == caption, "Incorrect menu caption");
    return menu;
}

static void invoke(IClassFactory *factory, const fs::path &path, const wchar_t *caption) {
    ComPtr<IContextMenu> menu;
    waitFor([&] { menu = query(factory, path, caption); return menu != nullptr; });
    CMINVOKECOMMANDINFO info{};
    info.cbSize = sizeof(info);
    info.fMask = CMIC_MASK_FLAG_NO_UI;
    info.lpVerb = MAKEINTRESOURCEA(0);
    info.nShow = SW_HIDE;
    check(SUCCEEDED(menu->InvokeCommand(&info)), "InvokeCommand failed");
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 3) return 1;
    if (FAILED(CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED))) return 2;
    auto module = LoadLibraryW(argv[1]);
    int code = 0;
    try {
        check(module != nullptr, "Cannot load extension DLL");
        auto get = reinterpret_cast<LPFNGETCLASSOBJECT>(GetProcAddress(module, "DllGetClassObject"));
        check(get != nullptr, "DllGetClassObject unavailable");
        ComPtr<IClassFactory> factory;
        check(SUCCEEDED(get(ClassId, IID_PPV_ARGS(&factory))), "Cannot get COM factory");
        const fs::path live(argv[2]);
        const auto backup = live.parent_path() / (live.filename().wstring() + L" - Copy");
        check(read(live / "progress.txt") == "before", "Expected fresh test data");
        check(!fs::exists(backup), "Expected a fresh test directory");
        invoke(factory.Get(), live, SaveCaption);
        waitFor([&] { return read(backup / "progress.txt") == "before"; });
        { std::ofstream file(live / "progress.txt"); file << "after\n"; }
        invoke(factory.Get(), backup, LoadCaption);
        waitFor([&] { return read(live / "progress.txt") == "before"; });
        // Wait until the host has committed the Load and released the game lock.
        waitFor([&] { return query(factory.Get(), live, SaveCaption) != nullptr; });
        query(factory.Get(), live.parent_path(), nullptr);
        bool recoveryFound = false;
        for (const auto &entry : fs::directory_iterator(live.parent_path())) {
            if (entry.path().filename().wstring().find(live.filename().wstring() + L".recovery-") == 0) {
                recoveryFound = true;
                query(factory.Get(), entry.path(), nullptr);
            }
        }
        check(recoveryFound, "Load did not retain a recovery folder");
        check(read(backup / "progress.txt") == "before", "Load changed its source backup");
        std::cout << "Explorer COM Save/Load roundtrip and menu eligibility passed\n";
    } catch (const std::exception &error) {
        std::cerr << error.what() << '\n';
        code = 3;
    }
    if (module) FreeLibrary(module);
    CoUninitialize();
    return code;
}
