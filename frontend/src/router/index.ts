import { createRouter, createWebHistory } from 'vue-router'
import Login from '@/views/Login.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'Login',
      component: Login,
      meta: { guest: true },
    },
    {
      path: '/register',
      name: 'Register',
      component: () => import('@/views/Register.vue'),
      meta: { guest: true },
    },
    {
      path: '/',
      component: () => import('@/components/AppLayout.vue'),
      meta: { requiresAuth: true },
      children: [
        {
          path: '',
          redirect: '/questions',
        },
        {
          path: 'questions',
          name: 'Questions',
          component: () => import('@/views/QuestionList.vue'),
        },
        {
          path: 'questions/new',
          name: 'QuestionNew',
          component: () => import('@/views/QuestionEdit.vue'),
        },
        {
          path: 'questions/:id',
          name: 'QuestionDetail',
          component: () => import('@/views/QuestionDetail.vue'),
        },
        {
          path: 'questions/:id/edit',
          name: 'QuestionEdit',
          component: () => import('@/views/QuestionEdit.vue'),
        },
        {
          path: 'review',
          name: 'Review',
          component: () => import('@/views/ReviewQueue.vue'),
        },
        {
          path: 'users',
          name: 'UserManagement',
          component: () => import('@/views/UserManagement.vue'),
          meta: { requiresAdmin: true },
        },
      ],
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'NotFound',
      component: () => import('@/views/NotFound.vue'),
    },
  ],
})

// 路由守卫
router.beforeEach(async (to, _from) => {
  const { useAuthStore } = await import('@/stores/auth')
  const auth = useAuthStore()

  if (to.meta.requiresAuth && !auth.isLoggedIn) {
    return { path: '/login', query: { redirect: to.fullPath } }
  }

  if (to.meta.guest && auth.isLoggedIn) {
    return '/questions'
  }

  if (to.meta.requiresAdmin && !auth.isAdmin) {
    return '/questions'
  }
})

export default router
