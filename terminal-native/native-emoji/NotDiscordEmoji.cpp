// System font rendering only. No downloaded emoji art or external runtime.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d2d1_2.h>
#include <dwrite.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdint>
#include <cstring>
using Microsoft::WRL::ComPtr;

struct RendererContext {
    HRESULT apartment;
    HRESULT ready;
    ComPtr<ID2D1Factory> d2d;
    ComPtr<IDWriteFactory> write;
    ComPtr<IWICImagingFactory> wic;
    RendererContext() noexcept : apartment(CoInitializeEx(nullptr, COINIT_MULTITHREADED)), ready(S_OK) {
        if (FAILED(apartment) && apartment != RPC_E_CHANGED_MODE) { ready = apartment; return; }
        ready = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, d2d.GetAddressOf());
        if (SUCCEEDED(ready)) ready = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory), reinterpret_cast<IUnknown**>(write.GetAddressOf()));
        if (SUCCEEDED(ready)) ready = CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(wic.GetAddressOf()));
    }
    ~RendererContext() noexcept { wic.Reset(); write.Reset(); d2d.Reset(); if (SUCCEEDED(apartment)) CoUninitialize(); }
};

extern "C" __declspec(dllexport) HRESULT __cdecl RenderSystemEmoji(const wchar_t* text, int length, int size, unsigned char* pixels, int capacity) noexcept {
    if (!text || !pixels || length < 1 || length > 64 || size < 16 || size > 192 || capacity < size * size * 4) return E_INVALIDARG;
    thread_local RendererContext context;
    if (FAILED(context.ready)) return context.ready;
    ComPtr<IWICBitmap> bitmap;
    HRESULT hr = context.wic->CreateBitmap(size, size, GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad, bitmap.GetAddressOf());
    ComPtr<ID2D1RenderTarget> target;
    if (SUCCEEDED(hr)) hr = context.d2d->CreateWicBitmapRenderTarget(bitmap.Get(), D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96.f, 96.f), target.GetAddressOf());
    ComPtr<IDWriteTextFormat> format;
    if (SUCCEEDED(hr)) hr = context.write->CreateTextFormat(L"Segoe UI Emoji", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size * .78f, L"en-us", format.GetAddressOf());
    if (SUCCEEDED(hr)) hr = format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    if (SUCCEEDED(hr)) hr = format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    if (SUCCEEDED(hr)) hr = format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    ComPtr<ID2D1SolidColorBrush> brush;
    if (SUCCEEDED(hr)) hr = target->CreateSolidColorBrush(D2D1::ColorF(D2D1::ColorF::White), brush.GetAddressOf());
    if (FAILED(hr)) return hr;
    target->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    target->BeginDraw();
    target->Clear(D2D1::ColorF(0.f, 0.f, 0.f, 0.f));
    target->DrawText(text, static_cast<UINT32>(length), format.Get(), D2D1::RectF(0, 0, static_cast<float>(size), static_cast<float>(size)), brush.Get(), D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
    hr = target->EndDraw();
    if (FAILED(hr)) return hr;
    WICRect rect { 0, 0, size, size };
    ComPtr<IWICBitmapLock> lock;
    hr = bitmap->Lock(&rect, WICBitmapLockRead, lock.GetAddressOf());
    if (FAILED(hr)) return hr;
    UINT stride = 0, bytes = 0; BYTE* source = nullptr;
    hr = lock->GetStride(&stride);
    if (SUCCEEDED(hr)) hr = lock->GetDataPointer(&bytes, &source);
    if (FAILED(hr) || !source || bytes < stride * static_cast<UINT>(size) || stride < static_cast<UINT>(size * 4)) return E_FAIL;
    for (int y = 0; y < size; y++) memcpy(pixels + y * size * 4, source + y * stride, size * 4);
    return S_OK;
}
