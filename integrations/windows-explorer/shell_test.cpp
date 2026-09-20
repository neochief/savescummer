#include <windows.h>
#include <shlobj.h>
#include <iostream>

int main(int argc, char **argv) {
    if (argc != 2) return 1;
    auto module = LoadLibraryA(argv[1]);
    if (!module) return 2;
    auto get = reinterpret_cast<LPFNGETCLASSOBJECT>(GetProcAddress(module, "DllGetClassObject"));
    auto unload = reinterpret_cast<LPFNCANUNLOADNOW>(GetProcAddress(module, "DllCanUnloadNow"));
    if (!get || !unload || unload() != S_OK) return 3;
    const CLSID clsid{0x3f8f42ce,0x463f,0x41b6,{0x98,0xd1,0x8c,0x8d,0x16,0xb8,0x89,0x31}};
    IClassFactory *factory = nullptr;
    if (FAILED(get(clsid, IID_IClassFactory, reinterpret_cast<void **>(&factory)))) return 4;
    if (unload() != S_FALSE) return 5;
    IContextMenu *menu = nullptr;
    if (FAILED(factory->CreateInstance(nullptr, IID_IContextMenu, reinterpret_cast<void **>(&menu)))) return 6;
    IShellExtInit *init = nullptr;
    if (FAILED(menu->QueryInterface(IID_IShellExtInit, reinterpret_cast<void **>(&init)))) return 7;
    if (init->Initialize(nullptr, nullptr, nullptr) != E_INVALIDARG) return 8;
    auto popup = CreatePopupMenu();
    if (menu->QueryContextMenu(popup, 0, 1, 100, CMF_NORMAL) != S_OK || GetMenuItemCount(popup) != 0) return 9;
    DestroyMenu(popup);
    init->Release(); menu->Release(); factory->Release();
    if (unload() != S_OK) return 10;
    FreeLibrary(module);
    std::cout << "Explorer COM lifetime and empty-selection checks passed\n";
    return 0;
}
