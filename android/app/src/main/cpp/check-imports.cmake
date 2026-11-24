# Fails the build if libhexspeech.so imports a call that starts, replaces or traces a process, or loads a shared object.
execute_process(COMMAND "${NM}" -D --undefined-only "${LIB}" OUTPUT_VARIABLE imports COMMAND_ERROR_IS_FATAL ANY)
foreach(name fork execlp execve prctl dlopen system)
    if(imports MATCHES " U ${name}(@|\n)")
        message(FATAL_ERROR "${LIB} imports ${name}")
    endif()
endforeach()
