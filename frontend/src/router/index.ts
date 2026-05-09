import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import Login from '@/views/Login.vue'
import Dashboard from '@/views/Dashboard.vue'

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
          component: Dashboard,
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
          path: 'knowledge-points',
          name: 'KnowledgePoints',
          component: () => import('@/views/KnowledgePoints.vue'),
        },
        {
          path: 'review',
          name: 'Review',
          component: () => import('@/views/ReviewQueue.vue'),
        },
        {
          path: 'papers',
          name: 'Papers',
          component: () => import('@/views/PaperList.vue'),
        },
        {
          path: 'papers/:id',
          name: 'PaperEdit',
          component: () => import('@/views/PaperEdit.vue'),
        },
        {
          path: 'users',
          name: 'UserManagement',
          component: () => import('@/views/UserManagement.vue'),
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
router.beforeEach((to, _from) => {
  const auth = useAuthStore()

  if (to.meta.requiresAuth && !auth.isLoggedIn) {
    return '/login'
  }

  if (to.meta.guest && auth.isLoggedIn) {
    return '/dashboard'
  }
})

export default router
