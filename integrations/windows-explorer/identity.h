#pragma once
#include <windows.h>

// Separate registrations allow development and normal hosts to coexist.
#ifdef SAVESCUMMER_EXPLORER_DEV
static const CLSID ClassId = {0x43bfba41,0xd0ab,0x44d3,{0xa5,0xd6,0x60,0x0e,0xb5,0xc7,0x4d,0x18}};
static constexpr auto SaveCaption = L"Save (dev)";
static constexpr auto LoadCaption = L"Load (dev)";
#else
static const CLSID ClassId = {0x3f8f42ce,0x463f,0x41b6,{0x98,0xd1,0x8c,0x8d,0x16,0xb8,0x89,0x31}};
static constexpr auto SaveCaption = L"Save";
static constexpr auto LoadCaption = L"Load";
#endif
