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
          redirect: '/dashboard',
        },
        {
          path: 'dashboard',
          name: 'Dashboard',
          component: () => import('@/views/Dashboard.vue'),
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
        {
          path: 'settings/tags',
          name: 'TagManagement',
          component: () => import('@/views/TagManagement.vue'),
          meta: { requiresAdmin: true },
        },
        {
          path: 'settings/knowledge-trees',
          name: 'KnowledgeTreeManagement',
          component: () => import('@/views/KnowledgeTreeManagement.vue'),
          meta: { requiresAdmin: true },
        },
        {
          path: 'profile',
          name: 'Profile',
          component: () => import('@/views/Profile.vue'),
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
    return '/dashboard'
  }

  if (to.meta.requiresAdmin && !auth.isAdmin) {
    return '/dashboard'
  }
})

export default router
