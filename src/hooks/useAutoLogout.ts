'use client';

import { useEffect, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { useAuthStore } from '@/store/authStore';
import { logger } from '@/utils/logger';

const INACTIVITY_TIMEOUT = 3 * 60 * 1000; // 3 分钟

/**
 * 用户无操作自动登出 Hook
 * - 监听 mousemove / keydown / click / scroll / touchstart
 * - 超过 3 分钟无操作自动登出并跳转到登录页
 * - 仅在登录状态下生效
 */
export function useAutoLogout() {
    const router = useRouter();
    const { isLoggedIn, logout } = useAuthStore();
    const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    useEffect(() => {
        if (!isLoggedIn) return;

        const resetTimer = () => {
            if (timerRef.current) {
                clearTimeout(timerRef.current);
            }
            timerRef.current = setTimeout(() => {
                logger.info('用户无操作超过 3 分钟，自动登出');
                logout();
                router.push('/login');
            }, INACTIVITY_TIMEOUT);
        };

        // 监听用户交互事件
        const events = ['mousemove', 'keydown', 'click', 'scroll', 'touchstart'];
        const handleActivity = () => resetTimer();

        events.forEach((event) => window.addEventListener(event, handleActivity));

        // 启动初始计时器
        resetTimer();

        return () => {
            events.forEach((event) => window.removeEventListener(event, handleActivity));
            if (timerRef.current) {
                clearTimeout(timerRef.current);
            }
        };
    }, [isLoggedIn, logout, router]);
}