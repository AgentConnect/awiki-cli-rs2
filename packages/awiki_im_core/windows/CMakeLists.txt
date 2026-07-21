# The Flutter tooling requires Visual Studio with CMake 3.14 or later. Keep
# this aligned with Flutter's Windows FFI plugin template.
cmake_minimum_required(VERSION 3.14)

set(PROJECT_NAME "awiki_im_core")
project(${PROJECT_NAME} LANGUAGES CXX)

if(NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
  message(FATAL_ERROR "awiki_im_core supports Windows x64 only.")
endif()

set(AWIKI_IM_CORE_LIBRARY
  "${CMAKE_CURRENT_SOURCE_DIR}/bin/awiki_im_core.dll"
)

if(NOT EXISTS "${AWIKI_IM_CORE_LIBRARY}")
  message(FATAL_ERROR
    "Missing ${AWIKI_IM_CORE_LIBRARY}. "
    "Run scripts/flutter/build-sdk-native.sh --windows-only before building "
    "a Flutter Windows application."
  )
endif()

set(awiki_im_core_bundled_libraries
  "${AWIKI_IM_CORE_LIBRARY}"
  PARENT_SCOPE
)
