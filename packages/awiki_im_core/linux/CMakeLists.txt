# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. Keep this value aligned with Flutter's FFI plugin template.
cmake_minimum_required(VERSION 3.10)

set(PROJECT_NAME "awiki_im_core")
project(${PROJECT_NAME} LANGUAGES CXX)

set(AWIKI_IM_CORE_LIBRARY
  "${CMAKE_CURRENT_SOURCE_DIR}/lib/libawiki_im_core.so"
)

if(EXISTS "${AWIKI_IM_CORE_LIBRARY}")
  set(awiki_im_core_bundled_libraries
    "${AWIKI_IM_CORE_LIBRARY}"
    PARENT_SCOPE
  )
else()
  message(WARNING
    "Missing ${AWIKI_IM_CORE_LIBRARY}. "
    "Run scripts/flutter/build-sdk-native.sh --linux-only before running "
    "AwikiImCore on Flutter Linux."
  )
  set(awiki_im_core_bundled_libraries
    ""
    PARENT_SCOPE
  )
endif()
