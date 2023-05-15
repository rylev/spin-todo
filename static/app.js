Vue.createApp({
    data() {
        return {
            todos: [],
            todo: '',
        }
    },

    created() {
        this.fetch()
    },

    methods: {
        async fetch() {
            let response = await fetch("/api/todos");
            let todos = await response.json();
            console.log("todos", todos);
            this.todos = todos;
        },

        async addTodo() {
            const description = this.todo && this.todo.trim();
            if (!description) {
                return;
            }

            await fetch("/api/todos/create", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                },
                body: JSON.stringify({ description }),
            });
            await this.fetch();

            this.todo = '';
        },

        async toggleComplete(todo) {
            const response = await fetch("/api/todos/" + todo.id, {
                method: "PATCH",
                body: JSON.stringify({ is_completed: !todo.is_completed })
            });
            await this.fetch();

        },

        async deleteTodo(todo) {
            await fetch("/api/todos/" + todo.id, {
                method: "DELETE",
            });
            await this.fetch();

        },

    },
}).mount('.todoapp');
