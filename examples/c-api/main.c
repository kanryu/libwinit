#include <stdio.h>
#include <string.h>

#include "winit.h"

typedef struct UserData {
  Window* window;
} UserData;

WinitControlFlow process_event(void* data, WinitEvent* event) {
    // rust owns `event`, no need to drop

    printf("New event\n");

    UserData* user_data = (UserData*) data;

    if (event -> event_type == WinitEventType_WindowEventCloseRequested) {
        winit_window_drop(user_data -> window);
        user_data -> window = NULL;
        return WinitControlFlow_Exit;
    }

    if (event -> event_type == WinitEventType_WindowEventResized) {
        printf("Window resized to: (%d x %d)\n", event -> window_resized.width, event -> window_resized.height);
    }

    if (event -> event_type == WinitEventType_WindowEventCursorMoved) {
        printf("Cursor moved to: (%f @ %f)\n", event -> cursor_moved.x, event -> cursor_moved.y);
    }

    if (event -> event_type == WinitEventType_WindowEventScaleFactorChanged) {
        printf("Scale factor changed to: %f. New physical size: (%d x %d)\n", event -> scale_factor.scale_factor, event -> scale_factor.width, event -> scale_factor.height);
    }

    if (event -> event_type == WinitEventType_MainEventsCleared) {
        //winit_window_request_redraw(user_data -> window);
    }

    return WinitControlFlow_Wait;
}

void with_title(WindowAttributes* window_attributes, char* title) {
    StringBox* title_string = boxer_string_from_byte_string((uint8_t*) title, strlen(title));
    winit_window_attributes_with_title(window_attributes, title_string);
    boxer_string_drop(title_string);
}

int main() {
    EventLoop* event_loop = winit_event_loop_new();

    WindowAttributes* window_attributes = winit_window_attributes_new();
    with_title(window_attributes, "Hello World");
    // Default logical window size
    winit_window_attributes_with_dimensions(window_attributes, 600.0, 400.0);

    Window* window = winit_create_window(event_loop, window_attributes);
    // Creating a window consumes the attributes object.

    struct UserData user_data;
    user_data.window = window;

    // Run until the window has received its Destroyed event.
    winit_event_loop_run_data(event_loop, (void*)&user_data, process_event);

    if (user_data.window != NULL) {
        winit_window_drop(user_data.window);
    }
    winit_event_loop_drop(event_loop);

    return 0;
}
