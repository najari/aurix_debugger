/*******************************************************************************
* Copyright (c) 2013-2022 Wind River, Inc. and others.
* All rights reserved. This program and the accompanying materials
* are made available under the terms of the Eclipse Public License v1.0
* and Eclipse Distribution License v1.0 which accompany this distribution.
* The Eclipse Public License is available at
* http://www.eclipse.org/legal/epl-v10.html
* and the Eclipse Distribution License is available at
* http://www.eclipse.org/org/documents/edl-v10.php.
* You may elect to redistribute this code under either of these licenses.
*
* Contributors:
*     Wind River - initial API and implementation
*******************************************************************************/

/*
 * Extension point definitions for stacktrace.c.
 *
 * TRACE_STACK_BOTTOM_CHECK - check stack trace bottom limit
 *
 * AURIX: a caller frame whose PC is 0 is the end of the chain (TriCore
 * reset/startup code runs with A11 = 0), not a frame to show.  Expanded
 * inside the trace_stack() loop, where 'down', 'stack' and 'ctx' exist.
 */

#define TRACE_STACK_BOTTOM_CHECK { \
        uint64_t bottom_pc_ = 0; \
        if (read_reg_value(&down, get_PC_definition(ctx), &bottom_pc_) == 0 && bottom_pc_ == 0) { \
            stack->complete = 1; \
            free_frame(&down); \
            break; \
        } \
    }
