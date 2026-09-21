#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <shlobj.h>
#include <shellapi.h>
#include <atomic>
#include <string>
#include <new>
#include "identity.h"

extern "C" unsigned sc_menu(const wchar_t *, size_t, void **);
extern "C" bool sc_invoke(const void *);
extern "C" void sc_menu_free(void *);

static std::atomic<long> objects{0};

class Menu final : public IShellExtInit, public IContextMenu {
    std::atomic<ULONG> refs_{1};
    std::wstring path_;
    void *menu_ = nullptr;
    unsigned kind_ = 0;
public:
    Menu() { ++objects; }
    ~Menu() { sc_menu_free(menu_); --objects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (iid == IID_IUnknown || iid == IID_IShellExtInit) *out = static_cast<IShellExtInit *>(this);
        else if (iid == IID_IContextMenu) *out = static_cast<IContextMenu *>(this);
        else return E_NOINTERFACE;
        AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs_; }
    ULONG STDMETHODCALLTYPE Release() override { auto n = --refs_; if (!n) delete this; return n; }
    HRESULT STDMETHODCALLTYPE Initialize(PCIDLIST_ABSOLUTE, IDataObject *data, HKEY) override {
        sc_menu_free(menu_); menu_ = nullptr; kind_ = 0; path_.clear();
        if (!data) return E_INVALIDARG;
        FORMATETC format{CF_HDROP, nullptr, DVASPECT_CONTENT, -1, TYMED_HGLOBAL};
        STGMEDIUM medium{};
        if (FAILED(data->GetData(&format, &medium))) return E_INVALIDARG;
        auto drop = static_cast<HDROP>(medium.hGlobal);
        if (DragQueryFileW(drop, 0xffffffff, nullptr, 0) == 1) {
            auto size = DragQueryFileW(drop, 0, nullptr, 0);
            if (size && size < 32768) {
                path_.resize(size + 1);
                DragQueryFileW(drop, 0, path_.data(), size + 1);
                path_.resize(size);
            }
        }
        ReleaseStgMedium(&medium);
        return path_.empty() ? E_INVALIDARG : S_OK;
    }
    HRESULT STDMETHODCALLTYPE QueryContextMenu(HMENU menu, UINT position, UINT first, UINT last, UINT flags) override {
        sc_menu_free(menu_); menu_ = nullptr; kind_ = 0;
        if ((flags & CMF_DEFAULTONLY) || path_.empty() || first > last) return MAKE_HRESULT(SEVERITY_SUCCESS,0,0);
        kind_ = sc_menu(path_.data(), path_.size(), &menu_);
        if (!kind_) return MAKE_HRESULT(SEVERITY_SUCCESS,0,0);
        if (!InsertMenuW(menu, position, MF_BYPOSITION | MF_STRING, first, kind_ == 1 ? SaveCaption : LoadCaption)) {
            sc_menu_free(menu_); menu_ = nullptr; kind_ = 0; return E_FAIL;
        }
        return MAKE_HRESULT(SEVERITY_SUCCESS,0,1);
    }
    HRESULT STDMETHODCALLTYPE InvokeCommand(CMINVOKECOMMANDINFO *info) override {
        if (!info || !menu_ || HIWORD(info->lpVerb) || LOWORD(info->lpVerb) != 0) return E_INVALIDARG;
        if (sc_invoke(menu_)) return S_OK;
        if (!(info->fMask & CMIC_MASK_FLAG_NO_UI)) {
            MessageBoxW(info->hwnd, L"The host could not confirm acceptance. Check SaveScummer for the operation result before trying again.", L"SaveScummer", MB_OK | MB_ICONERROR);
        }
        return E_FAIL;
    }
    HRESULT STDMETHODCALLTYPE GetCommandString(UINT_PTR id, UINT flags, UINT *, LPSTR name, UINT size) override {
        if (id || !kind_) return E_INVALIDARG;
        if (flags == GCS_VERBW) { lstrcpynW(reinterpret_cast<LPWSTR>(name), kind_ == 1 ? L"savescummer.save" : L"savescummer.load", size); return S_OK; }
        if (flags == GCS_VERBA) { lstrcpynA(name, kind_ == 1 ? "savescummer.save" : "savescummer.load", size); return S_OK; }
        return E_NOTIMPL;
    }
};
class Factory final : public IClassFactory {
    std::atomic<ULONG> refs_{1};
public:
    Factory() { ++objects; }
    ~Factory() { --objects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (iid != IID_IUnknown && iid != IID_IClassFactory) return E_NOINTERFACE;
        *out = static_cast<IClassFactory *>(this); AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs_; }
    ULONG STDMETHODCALLTYPE Release() override { auto n = --refs_; if (!n) delete this; return n; }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown *outer, REFIID iid, void **out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (outer) return CLASS_E_NOAGGREGATION;
        auto object = new(std::nothrow) Menu;
        if (!object) return E_OUTOFMEMORY;
        auto result = object->QueryInterface(iid, out); object->Release(); return result;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL lock) override { if (lock) ++objects; else --objects; return S_OK; }
};
STDAPI DllGetClassObject(REFCLSID clsid, REFIID iid, void **out) {
    if (!out) return E_POINTER;
    *out = nullptr;
    if (clsid != ClassId) return CLASS_E_CLASSNOTAVAILABLE;
    auto factory = new(std::nothrow) Factory;
    if (!factory) return E_OUTOFMEMORY;
    auto result = factory->QueryInterface(iid, out); factory->Release(); return result;
}
STDAPI DllCanUnloadNow() { return objects == 0 ? S_OK : S_FALSE; }
